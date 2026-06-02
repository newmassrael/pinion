//! `hello-card` — R757 §5.38 §5.40 Material 3 **cards** on the
//! pinion-shell substrate: the three M3 card *variants* — **elevated**,
//! **filled**, **outlined** — shown side by side as clickable surfaces.
//!
//! ## What a card is (and why it reuses the button interaction)
//!
//! An M3 card is a rounded **surface container** that groups related
//! content. The three variants differ only in how the surface is
//! lifted off the background:
//!
//! * **elevated** — a `Surface`-toned fill that casts a Material
//!   elevation shadow (resting Level 1, bumping to Level 2 on hover);
//! * **filled** — a higher-tone `SurfaceContainerHighest` fill, flat
//!   (no shadow), distinguished from the background by tone alone;
//! * **outlined** — a flat `Surface` fill ringed by a 1 px `Outline`
//!   border.
//!
//! A *clickable* card is one whose whole surface is the activation
//! target. Its pointer interaction — `enter → Hover`, `down → Pressed`,
//! `up-on-target → click → Hover`, `leave → Idle` — is **byte-identical**
//! to a button's, so the clickable card reuses
//! [`ButtonExternal`](pinion_core::widgets::button::ButtonExternal)
//! rather than declaring a second, identical SCXML statechart
//! ([[command-intent-wire-form-mirror]] / the R738 "same statechart ⇒
//! reuse the policy" rule). The card's distinct identity lives entirely
//! in its *paint* (the surface skin) and its *content* (the headline +
//! supporting text it wraps), not in a new interaction machine. WAI-ARIA
//! likewise announces a clickable card as a `button`, so the a11y nodes
//! are `Button` nodes carrying each card's headline as the accessible
//! name (name-from-contents).
//!
//! ## Composition (multi-external, the hello-accordion mould)
//!
//! N = 3 `ButtonExternal`s are composed through the R55.D.5
//! [`WidgetCore::create_extra_externals`] slot — the same homogeneous
//! cluster path `hello-accordion` (3 disclosures) and `settings-panel`
//! (6 checkboxes) use. Each card surface is tagged `card_{variant}`, so
//! the `InputRouter`'s depth-first walk routes a click on card *i*
//! straight to that card's handle; the three cards interact
//! independently with zero new interaction substrate.
//!
//! ## The card surface paint is inline (1st card-paint consumer)
//!
//! Following the inline-first rule ([[abstraction-needs-second-consumer]];
//! the R753 filter-chip / R756 input-chip precedent), the
//! [`card_surface_style`] skin is built **here**, in the single first
//! consumer, not pre-lifted into `pinion-widget-paint`. It composes the
//! already-shared SSOTs — the [`elevation`](pinion_widget_paint::elevation)
//! shadow ramp (R711) and the [`state_layer`](pinion_widget_paint::state_layer)
//! overlay (R752) — so the only genuinely new code is the variant →
//! `(base tone, shadow, border)` choice. When a 2nd card-surface consumer
//! appears the shared core lifts to `pinion_widget_paint::card`, exactly
//! as the chip body did at its 2nd consumer.
//!
//! Keyboard model (each card is its own Tab stop, like an accordion
//! header — a list of independent buttons, not a single roving group):
//! - `Space` / `Enter` activate the focused card (the button activation
//!   edge, emitting the `click` intent);
//! - `ArrowRight` / `ArrowDown` and `ArrowLeft` / `ArrowUp` rove focus to
//!   the next / previous card (wrapping), `Home` / `End` to the first /
//!   last — routed through the R664 [`pinion_core::focus_request`] mailbox
//!   so the move funnels through the same `FocusManager` path a mouse
//!   press uses.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::button::{button_a11y_state, read_button_state};
use pinion_widget_paint::{elevation, state_layer};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloCardRenderer, HelloCardRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 220;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across
/// the example gallery.
const THEME_TAG: &str = "app";

/// Card count — one per M3 variant.
const N: usize = 3;

/// M3 medium-shape card corner radius — 12 px (the spec value, larger
/// than the 8 px chip so the two surfaces read as different widgets).
const CARD_RADIUS: u32 = 12;
/// Outlined-card border width — 1 px.
const CARD_OUTLINE_W: u32 = 1;
/// Per-card surface size. Wide enough to hold a headline + supporting
/// line; uniform across variants so the three surface skins are the
/// only visible difference.
const CARD_W: u32 = 156;
const CARD_H: u32 = 132;
/// Inner padding inside each card surface (uniform inset).
const CARD_PAD: u32 = 16;

/// The three Material 3 card variants. The variant determines the
/// surface skin only ([`card_surface_style`]); the interaction machine
/// and content layout are shared.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CardVariant {
    /// `Surface`-toned fill that casts an elevation shadow.
    Elevated,
    /// Higher-tone `SurfaceContainerHighest` fill, flat.
    Filled,
    /// Flat `Surface` fill ringed by an `Outline` border.
    Outlined,
}

const VARIANTS: [CardVariant; N] =
    [CardVariant::Elevated, CardVariant::Filled, CardVariant::Outlined];

/// Per-card surface dispatch tags. `&'static str` (not `format!`)
/// because [`WidgetCore::focusable_tags`] returns `Vec<&'static str>`.
/// Each tag lands on one card's outer container, so a click hit-tests
/// straight to that card's `ButtonExternal`.
const CARD_TAGS: [&str; N] = ["card_elevated", "card_filled", "card_outlined"];

/// Card headlines — double as each card's AT accessible name (the first
/// text leaf, picked by name-from-contents enrichment).
const TITLES: [&str; N] = ["Elevated", "Filled", "Outlined"];

/// Supporting copy under each headline.
const BODIES: [&str; N] = [
    "Casts an elevation shadow.",
    "Higher-tone flat fill.",
    "Flat fill with an outline.",
];

/// Cached projection: one [`ButtonState`] per card, read from each
/// [`ButtonExternal`]'s `state` introspect slot. `Copy` so the shell
/// hands the snapshot into the paint closure without lifetime
/// gymnastics.
type CardStates = [ButtonState; N];

/// The Material elevation level an *elevated* card sits at for a given
/// interaction state: resting / pressed / disabled at Level 1, bumping
/// to Level 2 while hovered (the MD3 elevated-card hover lift). The
/// level → shadow mapping itself is the shared
/// [`elevation`](pinion_widget_paint::elevation) ramp — this fn only
/// picks the level.
const fn elevated_level(state: ButtonState) -> u8 {
    match state {
        ButtonState::Hover => 2,
        _ => 1,
    }
}

/// R757 §5.38 — the inline M3 card surface skin (1st card-paint
/// consumer). Builds the variant's resting `BoxStyle` and tints it
/// through the shared [`state_layer`](pinion_widget_paint::state_layer)
/// overlay for the current interaction `state`, so no card variant
/// re-derives the raw M3 state-layer opacity literals:
///
/// * **elevated** — `Surface` base + an [`elevation`] shadow at
///   [`elevated_level`];
/// * **filled** — `SurfaceContainerHighest` base, no shadow;
/// * **outlined** — `Surface` base + a 1 px `Outline` border.
///
/// The elevation ramp is the shared *Material-style* approximation, not
/// a claim of bit-exact MD3 dp values (see
/// [`elevation`](pinion_widget_paint::elevation)).
fn card_surface_style(variant: CardVariant, state: ButtonState, theme: &Theme) -> BoxStyle {
    let base = match variant {
        CardVariant::Filled => theme.resolve(ColorRole::SurfaceContainerHighest),
        CardVariant::Elevated | CardVariant::Outlined => theme.resolve(ColorRole::Surface),
    };
    let fill = state_layer::state_layer(base, state, theme);
    let mut style = BoxStyle::filled(fill).with_corner_radius(CARD_RADIUS);
    match variant {
        CardVariant::Elevated => {
            style = style.with_shadows(elevation::elevation(elevated_level(state)));
        }
        CardVariant::Outlined => {
            style = style
                .with_border(Border::new(theme.resolve(ColorRole::Outline), CARD_OUTLINE_W));
        }
        CardVariant::Filled => {}
    }
    style
}

/// Paint one card: an outer surface container (tagged `CARD_TAGS[i]`, the
/// click target) wrapping a column of the headline + supporting text.
fn view_card(i: usize, state: ButtonState, theme: &Theme) -> Scene {
    let title = Scene::Text(TextNode::styled(
        TITLES[i],
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let body = Scene::Text(TextNode::styled(
        BODIES[i],
        Rect::default(),
        TextStyle::new()
            .with_size_px(13)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![title, body])
            .with_tag(CARD_TAGS[i])
            .with_style(card_surface_style(VARIANTS[i], state, theme))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Start)
                    .with_gap(8)
                    .with_size(Size::px(CARD_W, CARD_H))
                    .with_padding(Rect::new(CARD_PAD, CARD_PAD, CARD_PAD, CARD_PAD)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping [`CardStates`] → [`Scene`]. A
/// centred row of the three variant cards.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &CardStates, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let cards: Vec<Scene> = (0..N).map(|i| view_card(i, state[i], &theme)).collect();
    let row = Scene::Container(
        ContainerNode::new(cards).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(20),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![row])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

pub struct CardView;

impl WidgetCore for CardView {
    type State = CardStates;
    // Every state change flows through `apply_key` (keyboard) or the
    // InputRouter's per-card `"send"` dispatch (pointer), never the
    // shell's enum-typed `keybinding` channel — so `()` satisfies the
    // trait's `Copy` bound without an unused event variant (mirror of
    // `hello-accordion` / `hello-radio-group`).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    /// Cards 1..N as sibling externals; card 0 is the primary
    /// [`Self::create_external`]. The substrate wraps the root as
    /// `Scene::Container([primary, ...extras])`, and the input router's
    /// depth-first walk dispatches each card click by tag.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        CARD_TAGS
            .iter()
            .skip(1)
            .map(|tag| ExtraExternal::new(*tag, Box::new(ButtonExternal::new())))
            .collect()
    }

    fn tag() -> &'static str {
        CARD_TAGS[0]
    }

    fn read_state(scene: &Scene) -> CardStates {
        let mut out: CardStates = [ButtonState::Idle; N];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = read_button_state(scene, CARD_TAGS[i]);
        }
        out
    }

    fn view(state: CardStates, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // No typed events reach the keybinding channel; the single
        // source of truth for the keymap lives in `apply_key`.
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-card (R757 §5.38 Material 3 cards — elevated / filled / outlined)"
    }

    /// Each card is its own Tab stop (a clickable card is a `button` in
    /// the document focus order), unlike a `RadioGroup`'s single-tab-stop
    /// roving model.
    fn focusable_tags() -> Vec<&'static str> {
        CARD_TAGS.to_vec()
    }

    /// Card keyboard model, gated on the focused card (roving-tabindex
    /// `apply_key` discipline — keys route only when one of our cards
    /// owns focus):
    ///
    /// - `Space` / `Enter` — activate the focused card (the button
    ///   activation edge, emitting the `click` intent);
    /// - `ArrowRight` / `ArrowDown` / `ArrowLeft` / `ArrowUp` /
    ///   `Home` / `End` — rove focus between the cards (wrapping).
    ///
    /// Both go through the shared
    /// [`pinion_core::focus_request::activate_or_rove`] SSOT (R760 lift —
    /// the mechanical "activate the focused tab stop, else rove" wiring
    /// `hello-accordion` + `hello-fab` also use). The card's *response* to
    /// `KeyboardActivate` (firing the `click` intent) lives in its
    /// `ButtonExternal` statechart. Rove focus moves are requested through
    /// the R664 [`pinion_core::focus_request`] mailbox; the shell drains it
    /// after this dispatch and applies it via the same `FocusManager` path
    /// a mouse press uses.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // R760 §5.39 — the mechanical "activate the focused tab stop with
        // KeyboardActivate, else rove shell focus" keyboard wiring is the
        // shared [`pinion_core::focus_request::activate_or_rove`] SSOT
        // (lifted at its 3rd consumer: accordion + card + fab). The card's
        // *response* to KeyboardActivate (firing the `click` intent) lives
        // in its ButtonExternal statechart, not in this wiring.
        pinion_core::focus_request::activate_or_rove(scene, CARD_TAGS.as_slice(), focused, key)
    }

    fn fmt_state_log(state: &CardStates) -> String {
        state
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{i}={}", s.as_name()))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl WidgetA11y for CardView {
    /// R757 §5.40 — flat contribution of N `button` nodes, one per
    /// clickable card. Each carries the interaction state flags and
    /// `focused` when that card owns shell focus, through the shared
    /// [`button_a11y_state`] SSOT (the same `ButtonState → AccessState`
    /// mapping the single-node `#[widget(role = Button)]` derive emits).
    /// No parent container node — WAI-ARIA defines no "card" role; a
    /// clickable card is a `button` in document order, and its
    /// accessible name comes from name-from-contents enrichment of the
    /// headline.
    fn access_node(state: &CardStates, focused: Option<&str>) -> Vec<AccessNode> {
        (0..N)
            .map(|i| {
                AccessNode::new(CARD_TAGS[i], AriaRole::Button)
                    .with_state(button_a11y_state(state[i], focused == Some(CARD_TAGS[i])))
            })
            .collect()
    }
}

impl WidgetView for CardView {
    type Renderer = HelloCardRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<CardView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::theme::Theme;

    fn idle() -> CardStates {
        [ButtonState::Idle; N]
    }

    fn with_state(idx: usize, s: ButtonState) -> CardStates {
        let mut out = idle();
        out[idx] = s;
        out
    }

    // ── view / composition ────────────────────────────────────────

    #[test]
    fn view_carries_primary_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<CardView>(
            idle(),
            &Frame::new(),
        );
    }

    #[test]
    fn view_carries_every_card_tag_and_headline() {
        let scene = pinion_core::Owner::new().run(|| view(&idle(), &Frame::new()));
        for (tag, title) in CARD_TAGS.iter().zip(TITLES) {
            assert!(scene.contains_tag(tag), "card surface tag {tag} present");
            assert!(scene_contains_text(&scene, title), "headline {title} painted");
        }
    }

    // ── card surface skin ─────────────────────────────────────────

    #[test]
    fn filled_card_base_differs_from_elevated_and_outlined() {
        // The filled variant is the only one that sits on the higher
        // SurfaceContainerHighest tone; elevated and outlined share the
        // plain Surface base. Idle isolates the resting base fill.
        let theme = Theme::light();
        let filled = card_surface_style(CardVariant::Filled, ButtonState::Idle, &theme).fill;
        let elevated = card_surface_style(CardVariant::Elevated, ButtonState::Idle, &theme).fill;
        let outlined = card_surface_style(CardVariant::Outlined, ButtonState::Idle, &theme).fill;
        assert_eq!(elevated, theme.resolve(ColorRole::Surface), "elevated rests on Surface");
        assert_eq!(outlined, theme.resolve(ColorRole::Surface), "outlined rests on Surface");
        assert_eq!(
            filled,
            theme.resolve(ColorRole::SurfaceContainerHighest),
            "filled rests on SurfaceContainerHighest",
        );
        assert_ne!(filled, elevated, "filled tone is distinct from the Surface base");
    }

    #[test]
    fn only_outlined_card_carries_a_border() {
        let theme = Theme::light();
        let idle = ButtonState::Idle;
        assert!(
            card_surface_style(CardVariant::Outlined, idle, &theme).border.is_some(),
            "outlined card carries the outline border",
        );
        assert!(card_surface_style(CardVariant::Elevated, idle, &theme).border.is_none());
        assert!(card_surface_style(CardVariant::Filled, idle, &theme).border.is_none());
    }

    #[test]
    fn only_elevated_card_casts_a_shadow_and_hover_bumps_it() {
        let theme = Theme::light();
        let idle = card_surface_style(CardVariant::Elevated, ButtonState::Idle, &theme);
        let hover = card_surface_style(CardVariant::Elevated, ButtonState::Hover, &theme);
        assert!(!idle.shadows.is_empty(), "elevated card casts a resting shadow");
        // Hover bumps Level 1 → Level 2, so the hovered shadow is larger.
        assert!(
            hover.shadows[0].blur > idle.shadows[0].blur,
            "hover bumps the elevation (larger blur)",
        );
        assert!(
            card_surface_style(CardVariant::Filled, ButtonState::Idle, &theme).shadows.is_empty(),
            "filled card is flat",
        );
        assert!(
            card_surface_style(CardVariant::Outlined, ButtonState::Idle, &theme).shadows.is_empty(),
            "outlined card is flat",
        );
    }

    #[test]
    fn hover_tints_every_variant_fill_through_the_state_layer() {
        let theme = Theme::light();
        for variant in VARIANTS {
            let idle = card_surface_style(variant, ButtonState::Idle, &theme).fill;
            let hover = card_surface_style(variant, ButtonState::Hover, &theme).fill;
            assert_ne!(idle, hover, "{variant:?} hover is tinted by the state layer");
        }
    }

    #[test]
    fn elevated_level_rests_at_one_hovers_at_two() {
        assert_eq!(elevated_level(ButtonState::Idle), 1);
        assert_eq!(elevated_level(ButtonState::Pressed), 1);
        assert_eq!(elevated_level(ButtonState::Disabled), 1);
        assert_eq!(elevated_level(ButtonState::Hover), 2);
    }

    // ── focus model ───────────────────────────────────────────────

    #[test]
    fn every_card_is_a_tab_stop() {
        assert_eq!(CardView::focusable_tags(), CARD_TAGS.to_vec());
    }

    #[test]
    fn arrow_right_requests_next_card_wrapping() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(CardView::apply_key(
            &mut scene,
            Some(CARD_TAGS[N - 1]),
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(CARD_TAGS[0]));
    }

    #[test]
    fn arrow_left_requests_previous_card_wrapping() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(CardView::apply_key(
            &mut scene,
            Some(CARD_TAGS[0]),
            "ArrowLeft",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(CARD_TAGS[N - 1]));
    }

    #[test]
    fn home_and_end_jump_to_first_and_last() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(CardView::apply_key(
            &mut scene,
            Some(CARD_TAGS[1]),
            "Home",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(CARD_TAGS[0]));
        assert!(CardView::apply_key(
            &mut scene,
            Some(CARD_TAGS[1]),
            "End",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(CARD_TAGS[N - 1]));
    }

    #[test]
    fn arrow_keys_ignored_when_no_card_focused() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(!CardView::apply_key(
            &mut scene,
            None,
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert!(!CardView::apply_key(
            &mut scene,
            Some("some_sibling"),
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain(), None, "no focus request emitted");
    }

    #[test]
    fn space_and_enter_activate_the_focused_card() {
        // `KeyboardActivate` is an internal SCXML transition (no visible
        // state change) whose witness is the emitted `click` intent;
        // that runtime walk-and-prefix path (`card_filled.click`) is
        // verified end-to-end by the r757 demo's `scene/intents`
        // assertion. At the unit level the contract is that the keymap
        // *accepts + routes* both activation keys to the focused card.
        for key in ["Space", "Enter"] {
            let mut scene = scene_fixture();
            assert!(
                CardView::apply_key(
                    &mut scene,
                    Some(CARD_TAGS[1]),
                    key,
                    pinion_core::Modifiers::empty(),
                ),
                "{key} activates the focused card",
            );
        }
    }

    // ── a11y ──────────────────────────────────────────────────────

    #[test]
    fn emits_n_button_nodes() {
        let nodes = CardView::access_node(&idle(), None);
        assert_eq!(nodes.len(), N);
        for node in &nodes {
            assert_eq!(node.role, AriaRole::Button);
            assert_eq!(node.state.checked, None, "a card carries no aria-checked");
        }
    }

    #[test]
    fn hover_and_pressed_states_set_interaction_flags() {
        let hovered = CardView::access_node(&with_state(0, ButtonState::Hover), None);
        assert!(hovered[0].state.hovered);
        assert!(!hovered[0].state.pressed);
        let pressed = CardView::access_node(&with_state(2, ButtonState::Pressed), None);
        assert!(pressed[2].state.pressed);
        assert!(!pressed[2].state.hovered);
    }

    #[test]
    fn focused_card_marks_only_that_node_focused() {
        let nodes = CardView::access_node(&idle(), Some(CARD_TAGS[1]));
        assert!(!nodes[0].state.focused);
        assert!(nodes[1].state.focused);
        assert!(!nodes[2].state.focused);
    }

    #[test]
    fn names_come_from_each_card_headline() {
        let state = idle();
        let scene = pinion_core::Owner::new().run(|| view(&state, &Frame::new()));
        let mut nodes = CardView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        for (i, node) in nodes.iter().enumerate() {
            assert_eq!(node.name.as_deref(), Some(TITLES[i]));
        }
    }

    // ── helpers ───────────────────────────────────────────────────

    fn scene_contains_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content == needle,
            Scene::Container(c) => c.children.iter().any(|c| scene_contains_text(c, needle)),
            _ => false,
        }
    }

    /// Build a fresh card scene mirroring the shell's boot composition
    /// (`Scene::Container([primary, ...extras])`) so `apply_key` walks
    /// the live topology.
    fn scene_fixture() -> Scene {
        use pinion_core::scene::ExternalNode;
        let primary = Scene::External(
            ExternalNode::new(CardView::create_external()).with_tag(CARD_TAGS[0]),
        );
        let mut children = vec![primary];
        for extra in CardView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag.into_owned()),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
