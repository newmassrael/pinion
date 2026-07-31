//! `hello-snackbar` — R725 §5.28 §5.38 §5.40 consumer of the Snackbar
//! timed-auto-dismiss substrate + the first `inversePrimary` consumer.
//!
//! ## What this demonstrates
//!
//! A **Show** button raises a Material 3 *snackbar*: a transient,
//! bottom-anchored message styled with the R723 inverse tier
//! (`inverseSurface` fill, `inverseOnSurface` text) and a single
//! `inversePrimary` **UNDO** action — the first widget to render
//! `ColorRole::InversePrimary`, clearing the R723 "inversePrimary has
//! no consumer" carry.
//!
//! The snackbar **auto-dismisses after 4 s** via the §5.28
//! [`SnackbarTimer`]
//! `Tickable` (the [`CaretBlink`] one-shot-countdown sibling). Because
//! the dismissal rides the animation driver, it is **deterministically
//! drivable** by the R724 `scene/tick` RPC: a client shows the
//! snackbar, `scene/tick`s past 4 s, and observes the auto-dismiss
//! without waiting on wall-clock time. UNDO dismisses early and bumps
//! an undo counter.
//!
//! ## Architecture (unidirectional, no reducer→widget back-channel)
//!
//! Two [`ButtonExternal`]s (Show trigger + UNDO action) emit
//! `"<tag>.click"` intents; [`SnackbarView::update`] maps them onto the
//! `SnackbarTimer` (show / dismiss) and an undo-count [`Signal`]. The
//! view reads `timer.visible()` to push the overlay. Data flows input →
//! External → intent → reducer → timer/Signal → view.
//!
//! ## Verification
//!
//! `tools/demos/r725_snackbar.py`: show → snackbar visible (inverse
//! tones + inversePrimary action + `role=status`), `scene/tick(0.5)`
//! still visible, `scene/tick(4.0)` auto-dismissed (deterministic);
//! show → UNDO → dismissed + count bumped; live-pixel inverseSurface.
//!
//! [`CaretBlink`]: pinion_core::widgets::caret_blink::CaretBlink

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::snackbar::{
    SnackbarTimer, snackbar_introspection_extra, use_snackbar_timer,
};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, read_button_focused, read_button_state,
};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSnackbarRenderer, HelloSnackbarRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 300;
const THEME_TAG: &str = "app";

const SHOW_TAG: &str = "show_snack";
const UNDO_TAG: &str = "snack_undo";
const SNACK_TAG: &str = "snackbar";
/// R810 §5.38 §5.12 — tag of the query-only `SnackbarIntrospect`
/// extra-external (distinct from the paint container [`SNACK_TAG`], the
/// `dialog_state` vs `dialog` split the modal bindings use). An AI agent
/// queries `snackbar_state.visible` / `.remaining` / `.duration` over
/// the §5.12 RPC plane instead of inferring shown-state from paint.
const SNACK_STATE_TAG: &str = "snackbar_state";
const SHOW_HOVER_KEY: &str = "show_hover";
const UNDO_HOVER_KEY: &str = "undo_hover";

const SNACK_W: u32 = 320;
const SNACK_H: u32 = 48;
const SNACK_PAD: u32 = 16;
const SNACK_MESSAGE: &str = "Message sent";

/// Material 3 short snackbar duration (with action). The auto-dismiss
/// horizon the Show button passes to [`SnackbarTimer::show`].
const SNACK_DURATION_SECS: f32 = 4.0;

/// `Owner::cache` key for the undo-count `Signal`.
const UNDO_COUNT_KEY: &str = "snack.undo_count";

/// The snackbar's timed-dismiss driver for this view scope (registers
/// the `Tickable` once). Shared by the reducer (show / dismiss), the
/// view (visibility), and `access_node` (whether the live region is
/// present).
fn snackbar() -> Rc<SnackbarTimer> {
    use_snackbar_timer(SNACK_TAG)
}

/// `Owner::cache`-keyed undo counter — bumped each time UNDO dismisses
/// the snackbar, surfaced in the status line + introspection.
fn undo_count() -> Rc<Signal<u32>> {
    Owner::current()
        .expect("undo_count requires an active Owner scope")
        .cache(UNDO_COUNT_KEY, || Signal::new(0u32))
}

/// Render one button via the [`pinion_widget_paint::button`] substrate.
#[allow(clippy::too_many_arguments)] // R1020: one arg per paint axis + focusable opt-in
fn button_scene(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    hover_key: &'static str,
    colors: &ButtonColors,
    size: Size,
    focusable: bool,
) -> Scene {
    // R727 — delegates to the lifted SSOT core; the snackbar supplies
    // its own action tone (`colors`) + 15 px font.
    pinion_widget_paint::button::button_scene(
        label,
        state,
        hover_key,
        colors,
        &ButtonStyle::m3_default(tag)
            .with_size(size)
            .with_label_font_size_px(15)
            .with_focusable(focusable),
    )
}

/// R725 §5.50 — the snackbar's UNDO action button colours: an M3 text
/// action on the `inverseSurface` snackbar — `inversePrimary` label,
/// `inverseSurface` base so the button blends into the snackbar body
/// (text-button look). This is the first `inversePrimary` consumer.
fn snackbar_action_colors(theme: &pinion_core::theme::Theme) -> ButtonColors {
    let inverse_surface = theme.resolve(ColorRole::InverseSurface);
    let inverse_primary = theme.resolve(ColorRole::InversePrimary);
    ButtonColors::new(
        inverse_surface,                            // base (blends into snackbar)
        inverse_primary,                            // hover / pressed state layer
        inverse_surface,                            // disabled fill (unused here)
        inverse_primary,                            // label
        theme.resolve(ColorRole::InverseOnSurface), // disabled label
    )
}

/// Build the bottom-anchored snackbar overlay (only called while
/// visible): an `inverseSurface` bar holding the message
/// (`inverseOnSurface`) and the `inversePrimary` UNDO action.
fn snackbar_overlay(undo_state: ButtonState, theme: &pinion_core::theme::Theme) -> Scene {
    let message = Scene::Text(TextNode::styled(
        SNACK_MESSAGE,
        Rect::default(),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::InverseOnSurface)),
    ));
    let undo = button_scene(
        UNDO_TAG,
        "UNDO",
        undo_state,
        UNDO_HOVER_KEY,
        &snackbar_action_colors(theme),
        Size::px(72, 32),
        false,
    );
    let left = (WIN_W - SNACK_W) / 2;
    let top = WIN_H - SNACK_H - SNACK_PAD;
    Scene::Container(
        ContainerNode::new(vec![message, undo])
            .with_tag(SNACK_TAG)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::InverseSurface)).with_corner_radius(4),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(left, top)
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::SpaceBetween)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(SNACK_W, SNACK_H))
                    .with_padding(Rect::new(SNACK_PAD, SNACK_PAD, SNACK_PAD, SNACK_PAD)),
            ),
    )
}

/// Cached posture of the two buttons + their focus flags
/// (`[show, undo]`). The snackbar visibility + undo count live in the
/// timer / Signal the owner-scoped view + `access_node` read directly.
type SnackbarViewState = (ButtonState, ButtonState, [bool; 2]);

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`,
/// reading the reactive `SnackbarTimer` + undo-count signal.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: SnackbarViewState, _frame: &Frame) -> Scene {
    let (show_state, undo_state, _focus) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let visible = snackbar().visible();
    let undos = undo_count().get();

    let show = button_scene(
        SHOW_TAG,
        "Send message",
        show_state,
        SHOW_HOVER_KEY,
        &ButtonColors::filled_tonal(&theme),
        Size::px(160, 40),
        true,
    );
    let status = Scene::Text(TextNode::styled(
        format!("Undone {undos} time(s)"),
        Rect::default(),
        TextStyle::new()
            .with_size_px(13)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    let content = Scene::Container(
        ContainerNode::new(vec![show, status]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_flex_grow(1.0)
                .with_gap(14),
        ),
    );

    let mut children = vec![content];
    if visible {
        children.push(snackbar_overlay(undo_state, &theme));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

struct SnackbarView;

impl WidgetCore for SnackbarView {
    type State = SnackbarViewState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // R810 §5.38 §5.12 — register the query-only snackbar introspect
        // node next to the action button, resolving the *same*
        // `Rc<SnackbarTimer>` the view-fn reads (one SSOT) so an AI agent
        // can query the live shown-state + countdown over RPC.
        vec![
            ExtraExternal::new(UNDO_TAG, Box::new(ButtonExternal::new())),
            snackbar_introspection_extra(SNACK_STATE_TAG, snackbar()),
        ]
    }

    fn tag() -> &'static str {
        SHOW_TAG
    }

    fn read_state(scene: &Scene) -> SnackbarViewState {
        (
            read_button_state(scene, SHOW_TAG),
            read_button_state(scene, UNDO_TAG),
            [
                read_button_focused(scene, SHOW_TAG),
                read_button_focused(scene, UNDO_TAG),
            ],
        )
    }

    fn view(state: SnackbarViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-snackbar (R725 §5.28 §5.38 §5.40)"
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
        apply_aria_activate(scene, focused, key, SHOW_TAG)
    }

    /// R725 reducer: the Show button raises the snackbar (restarting the
    /// 4 s countdown); UNDO dismisses early and bumps the undo counter.
    /// Both are owner-scoped side effects on the timer / Signal — no
    /// back-channel into a widget.
    fn update(
        _state: SnackbarViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "show_snack.click" => snackbar().show(SNACK_DURATION_SECS),
            "snack_undo.click" => {
                snackbar().dismiss();
                let c = undo_count();
                c.set(c.get() + 1);
            }
            _ => {}
        }
        Vec::new()
    }
}

impl WidgetA11y for SnackbarView {
    /// Show button always; while the snackbar is visible, a passive
    /// `role=status` live region (its accessible name derived from the
    /// message `TextNode` by `enrich_names_from_scene`) owning the UNDO
    /// [`AriaRole::Button`] action.
    fn access_node(state: &SnackbarViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let show = AccessNode::new(SHOW_TAG, AriaRole::Button)
            .with_state(button_a11y_state(state.0, focused == Some(SHOW_TAG)));
        if !snackbar().visible() {
            return vec![show];
        }
        let region = AccessNode::new(SNACK_TAG, AriaRole::Status).with_child(UNDO_TAG);
        vec![
            show,
            region,
            AccessNode::new(UNDO_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.1, focused == Some(UNDO_TAG))),
        ]
    }
}

impl WidgetView for SnackbarView {
    type Renderer = HelloSnackbarRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<SnackbarView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> SnackbarViewState {
        (ButtonState::Idle, ButtonState::Idle, [false, false])
    }

    fn rendered() -> Scene {
        let owner = Owner::new();
        owner.run(|| view(idle(), &Frame::new()))
    }

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_container(ch, tag)),
            _ => None,
        }
    }

    #[test]
    fn snackbar_hidden_until_shown() {
        let scene = rendered();
        assert!(
            find_container(&scene, SNACK_TAG).is_none(),
            "hidden by default"
        );
    }

    #[test]
    fn show_makes_snackbar_visible_with_inverse_tones() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            snackbar().show(SNACK_DURATION_SECS);
            view(idle(), &Frame::new())
        });
        let theme = pinion_core::theme::Theme::light();
        let snack = find_container(&scene, SNACK_TAG).expect("snackbar visible after show");
        assert_eq!(snack.style.fill, theme.resolve(ColorRole::InverseSurface));
    }

    #[test]
    fn tick_auto_dismisses_snackbar() {
        let owner = Owner::new();
        owner.run(|| {
            snackbar().show(SNACK_DURATION_SECS);
        });
        // Advance the registered Tickable past the duration.
        owner.tick_animations(SNACK_DURATION_SECS + 0.1);
        let scene = owner.run(|| view(idle(), &Frame::new()));
        assert!(
            find_container(&scene, SNACK_TAG).is_none(),
            "auto-dismissed after duration"
        );
    }

    #[test]
    fn r725_show_reports_button_role() {
        let owner = Owner::new();
        let nodes = owner.run(|| <SnackbarView as WidgetA11y>::access_node(&idle(), None));
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert_eq!(nodes[0].tag, SHOW_TAG);
    }

    #[test]
    fn r725_visible_snackbar_is_status_live_region() {
        let owner = Owner::new();
        let nodes = owner.run(|| {
            snackbar().show(SNACK_DURATION_SECS);
            <SnackbarView as WidgetA11y>::access_node(&idle(), None)
        });
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == SNACK_TAG && n.role == AriaRole::Status),
            "snackbar exposes a role=status live region while visible"
        );
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == UNDO_TAG && n.role == AriaRole::Button),
            "UNDO action is a button"
        );
    }
}
