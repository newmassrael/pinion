//! `hello-fab` — R760 §5.38 §5.40 §5.50 — the Material 3 **Floating
//! Action Button** in its four size variants — **small**, **standard**,
//! **large**, **extended** — shown side by side as clickable surfaces.
//!
//! ## A FAB is an elevated button (reuse, don't re-invent)
//!
//! An M3 FAB is a high-emphasis action button distinguished from an
//! ordinary button by three things only: a **primary (accent) container
//! tone**, a **resting elevation** (it floats above the content, lifting
//! further on hover), and a **larger rounded-square shape**. Its pointer
//! interaction — `enter → Hover`, `down → Pressed`, `up-on-target →
//! click → Hover`, `leave → Idle` — is **byte-identical** to a button's,
//! so the FAB reuses [`ButtonExternal`](pinion_core::widgets::button) (the
//! R738 "same statechart ⇒ reuse the policy" rule, as `hello-card` did).
//!
//! The *paint* likewise reuses the M3 button substrate rather than
//! hand-rolling a parallel surface ([[use-substrate-not-hand-rolled-equivalent]]):
//! [`view_button`] already composes the
//! [`m3_button_fill`](pinion_widget_paint::button::m3_button_fill)
//! state-layer matrix + the centred label + the focus ring + geometry, and
//! [`ButtonColors::accent`] is exactly the FAB's accent-container scheme
//! (`base = Accent`, `state_layer = OnAccent`). The one axis a FAB adds —
//! the **elevation shadow** — is the `elevation_level` axis the
//! [`ButtonStyle`] type doc anticipated, added to the shared substrate in
//! R760 (additive, default `0` = every existing flat button unchanged).
//! So a FAB is `view_button` + `ButtonColors::accent` + a rounded-square
//! [`ButtonStyle`] at elevation 3 — **zero new paint code here**.
//!
//! WAI-ARIA announces a FAB as a `button`; an icon-only FAB has no text to
//! enrich a name from, so each node carries an **explicit accessible
//! name** (the action: "Add" / "Edit" / "Create" / "Compose"), which
//! `enrich_names_from_scene` leaves intact (it only fills *missing* names).
//!
//! ## Composition + keyboard (the hello-card mould)
//!
//! N = 4 `ButtonExternal`s are composed through the R55.D.5
//! [`WidgetCore::create_extra_externals`] slot, each surface tagged
//! `fab_{variant}`. Each FAB is its own Tab stop; `Space` / `Enter`
//! activates the focused FAB and the Arrow / `Home` / `End` keys rove
//! between them — the shared
//! [`pinion_core::focus_request::activate_or_rove`] SSOT (lifted R760 at
//! its 3rd consumer: accordion + card + fab).

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::button::{
    button_a11y_state, read_button_state, view_button, ButtonColors, ButtonStyle,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFabRenderer, HelloFabRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 260;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across the
/// example gallery.
const THEME_TAG: &str = "app";

/// FAB count — one per M3 size variant.
const N: usize = 4;
const EXTENDED: usize = 3;

/// Per-FAB surface tags (the click target + a11y tag + paint root).
const FAB_TAGS: [&str; N] = ["fab_small", "fab_standard", "fab_large", "fab_extended"];
/// Captions beneath each FAB naming the variant.
const CAPTIONS: [&str; N] = ["Small", "Standard", "Large", "Extended"];
/// Explicit accessible names (the action each FAB performs) — an icon-only
/// FAB has no text to enrich from, so these are authoritative.
const NAMES: [&str; N] = ["Add", "Edit", "Create", "Compose"];

/// Per-variant FAB edge size (square; the extended FAB's width is fixed
/// wider to hold its label, see [`fab_size`]).
const SMALL: u32 = 40;
const STANDARD: u32 = 56;
const LARGE: u32 = 96;
const EXTENDED_W: u32 = 168;
/// M3 FAB corner radii: small 12, standard 16, large 28, extended 16.
const RADII: [u32; N] = [12, 16, 28, 16];
/// Icon / label font size per variant.
const FONTS: [u32; N] = [18, 24, 36, 16];
/// FAB icon (a stand-in "add" glyph, ASCII so no font-coverage risk) and
/// the extended FAB's icon + label.
const ICON: &str = "+";
const EXTENDED_LABEL: &str = "+  Compose";

/// Cached projection: one [`ButtonState`] per FAB.
type FabStates = [ButtonState; N];

/// The Material elevation level a FAB sits at for a given interaction
/// state: resting at Level 3, lifting to Level 4 while hovered (the M3 FAB
/// hover raise); pressed / disabled stay at the resting level. The
/// level → shadow mapping is the shared
/// [`elevation`](pinion_widget_paint::elevation) ramp — this only picks
/// the level.
const fn fab_level(state: ButtonState) -> u8 {
    match state {
        ButtonState::Hover => 4,
        _ => 3,
    }
}

/// The FAB's painted size: a square for the icon variants, a fixed wider
/// pill for the extended variant.
fn fab_size(i: usize) -> Size {
    match i {
        0 => Size::px(SMALL, SMALL),
        1 => Size::px(STANDARD, STANDARD),
        2 => Size::px(LARGE, LARGE),
        _ => Size::px(EXTENDED_W, STANDARD),
    }
}

/// Paint one FAB by reusing [`view_button`] with the accent colour scheme
/// and the new elevation axis. The icon variants pass the `"+"` glyph as
/// the (centred) label; the extended variant passes its icon + label.
fn view_fab(i: usize, state: ButtonState, focused: bool, theme: &Theme) -> Scene {
    let colors = ButtonColors::accent(theme);
    // Discrete hover (no spring): 1.0 on Hover, 0.0 otherwise — m3_button_fill
    // lands on the same endpoints a spring would settle to.
    let hover_progress = if matches!(state, ButtonState::Hover) { 1.0 } else { 0.0 };
    let style = ButtonStyle::m3_default(FAB_TAGS[i])
        .with_corner_radius(RADII[i])
        .with_size(fab_size(i))
        .with_label_font_size_px(FONTS[i])
        // R1020 §5.39 — each FAB is a Tab stop; opt it into the
        // scene-derived focus enumeration.
        .with_focusable(true)
        .with_elevation(fab_level(state));
    let label = if i == EXTENDED { EXTENDED_LABEL } else { ICON };
    view_button(label, state, hover_progress, focused, &colors, &style)
}

/// view-fn (§6.3): pure sync mapping [`FabStates`] → [`Scene`]. A centred
/// row of the four FAB variants, each over a caption.
///
/// Painted with `focused = false` (no keyboard-focus ring): the shell's
/// `view(state, frame)` carries no focus context, so — like `hello-card`
/// and the R694 derive path — the FAB defers the *painted* ring (the R690
/// shared shell-focus-paint carry) and reports focus through the a11y
/// tree (`access_node` receives `focused`) instead.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &FabStates, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let columns: Vec<Scene> = (0..N)
        .map(|i| {
            let fab = view_fab(i, state[i], false, &theme);
            let caption = Scene::Text(TextNode::styled(
                CAPTIONS[i],
                Rect::default(),
                TextStyle::new()
                    .with_size_px(13)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            ));
            Scene::Container(
                ContainerNode::new(vec![fab, caption]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Column)
                        .with_align_items(AlignItems::Center)
                        .with_gap(12),
                ),
            )
        })
        .collect();
    let row = Scene::Container(
        ContainerNode::new(columns).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(32),
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

pub struct FabView;

impl WidgetCore for FabView {
    type State = FabStates;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        FAB_TAGS
            .iter()
            .skip(1)
            .map(|tag| ExtraExternal::new(*tag, Box::new(ButtonExternal::new())))
            .collect()
    }

    fn tag() -> &'static str {
        FAB_TAGS[0]
    }

    fn read_state(scene: &Scene) -> FabStates {
        let mut out: FabStates = [ButtonState::Idle; N];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = read_button_state(scene, FAB_TAGS[i]);
        }
        out
    }

    fn view(state: FabStates, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-fab (R760 §5.38 Material 3 FAB — small / standard / large / extended)"
    }


    /// R760 §5.39 — `Space` / `Enter` activates the focused FAB; the Arrow
    /// / `Home` / `End` keys rove between them — the shared
    /// [`pinion_core::focus_request::activate_or_rove`] SSOT (the FAB is
    /// its 3rd consumer, after accordion + card).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::focus_request::activate_or_rove(scene, FAB_TAGS.as_slice(), focused, key)
    }

    fn fmt_state_log(state: &FabStates) -> String {
        state
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{i}={}", s.as_name()))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl WidgetA11y for FabView {
    /// R760 §5.40 — N `button` nodes, one per FAB, each carrying its
    /// interaction state flags + `focused`, and an **explicit** accessible
    /// name (the action — an icon-only FAB has no text to enrich from).
    fn access_node(state: &FabStates, focused: Option<&str>) -> Vec<AccessNode> {
        (0..N)
            .map(|i| {
                AccessNode::new(FAB_TAGS[i], AriaRole::Button)
                    .with_name(NAMES[i])
                    .with_state(button_a11y_state(state[i], focused == Some(FAB_TAGS[i])))
            })
            .collect()
    }
}

impl WidgetView for FabView {
    type Renderer = HelloFabRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<FabView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> FabStates {
        [ButtonState::Idle; N]
    }

    fn scene_fixture() -> Scene {
        use pinion_core::scene::ExternalNode;
        let primary =
            Scene::External(ExternalNode::new(FabView::create_external()).with_tag(FAB_TAGS[0]));
        let mut children = vec![primary];
        for extra in FabView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag.into_owned()),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }

    // ── view / composition ────────────────────────────────────────────

    #[test]
    fn view_carries_primary_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<FabView>(idle(), &Frame::new());
    }

    #[test]
    fn view_carries_every_fab_tag_and_caption() {
        let scene = pinion_core::Owner::new().run(|| view(&idle(), &Frame::new()));
        for (tag, caption) in FAB_TAGS.iter().zip(CAPTIONS) {
            assert!(scene.contains_tag(tag), "fab surface tag {tag} present");
            assert!(scene_contains_text(&scene, caption), "caption {caption} painted");
        }
    }

    // ── FAB surface (reused view_button) ───────────────────────────────

    #[test]
    fn every_fab_rests_on_the_accent_tone() {
        let theme = Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        for i in 0..N {
            let scene = view_fab(i, ButtonState::Idle, false, &theme);
            let Scene::Container(c) = &scene else { panic!("Container") };
            assert_eq!(c.style.fill, accent, "FAB {i} idle fill is the accent container tone");
        }
    }

    #[test]
    fn every_fab_casts_a_resting_shadow_and_hover_lifts_it() {
        let theme = Theme::light();
        for i in 0..N {
            let idle = view_fab(i, ButtonState::Idle, false, &theme);
            let hover = view_fab(i, ButtonState::Hover, false, &theme);
            let (Scene::Container(ci), Scene::Container(ch)) = (&idle, &hover) else {
                panic!("Container");
            };
            assert!(!ci.style.shadows.is_empty(), "FAB {i} casts a resting shadow (elevated)");
            assert!(
                ch.style.shadows[0].blur > ci.style.shadows[0].blur,
                "FAB {i} hover lifts the elevation (L3 -> L4, larger blur)",
            );
        }
    }

    #[test]
    fn fab_level_rests_at_three_hovers_at_four() {
        assert_eq!(fab_level(ButtonState::Idle), 3);
        assert_eq!(fab_level(ButtonState::Pressed), 3);
        assert_eq!(fab_level(ButtonState::Disabled), 3);
        assert_eq!(fab_level(ButtonState::Hover), 4);
    }

    #[test]
    fn corner_radii_follow_the_m3_size_ramp() {
        // small 12 < standard 16 = extended 16 < large 28.
        assert_eq!(RADII, [12, 16, 28, 16]);
        let theme = Theme::light();
        let small = view_fab(0, ButtonState::Idle, false, &theme);
        let Scene::Container(c) = &small else { panic!("Container") };
        assert_eq!(c.style.corner_radius, 12);
    }

    #[test]
    fn read_state_reflects_a_send_driven_hover() {
        let mut scene = scene_fixture();
        if let Some(node) = scene.find_external_with_tag_mut(FAB_TAGS[1]) {
            node.handle
                .introspect_mut()
                .expect("opted in")
                .invoke(
                    "send",
                    pinion_core::external::IntrospectValue::Text("PointerEnter".to_string()),
                )
                .expect("PointerEnter accepted");
        }
        let state = FabView::read_state(&scene);
        assert_eq!(state[1], ButtonState::Hover, "the entered FAB reads Hover");
        assert_eq!(state[0], ButtonState::Idle, "siblings untouched");
    }

    // ── focus / keyboard (shared activate_or_rove) ─────────────────────

    #[test]
    fn every_fab_is_a_tab_stop() {
        // §5.39: tree order IS tab order — collected from the paint scene.
        let scene = pinion_core::Owner::new().run(|| view(&idle(), &Frame::new()));
        assert_eq!(
            scene.collect_focusable_tags(),
            FAB_TAGS.iter().map(|t| (*t).to_owned()).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn arrow_right_roves_to_the_next_fab_wrapping() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(FabView::apply_key(
            &mut scene,
            Some(FAB_TAGS[N - 1]),
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(FAB_TAGS[0]));
    }

    #[test]
    fn space_activates_the_focused_fab() {
        let mut scene = scene_fixture();
        assert!(FabView::apply_key(
            &mut scene,
            Some(FAB_TAGS[1]),
            "Space",
            pinion_core::Modifiers::empty(),
        ));
    }

    #[test]
    fn arrow_keys_ignored_when_no_fab_focused() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(!FabView::apply_key(
            &mut scene,
            None,
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain(), None);
    }

    fn scene_contains_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content == needle,
            Scene::Container(c) => c.children.iter().any(|c| scene_contains_text(c, needle)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn idle() -> FabStates {
        [ButtonState::Idle; N]
    }

    #[test]
    fn emits_n_button_nodes_with_explicit_names() {
        let nodes = FabView::access_node(&idle(), None);
        assert_eq!(nodes.len(), N);
        for (node, name) in nodes.iter().zip(NAMES) {
            assert_eq!(node.role, AriaRole::Button);
            assert_eq!(node.name.as_deref(), Some(name), "icon FAB carries an explicit name");
        }
    }

    #[test]
    fn explicit_names_survive_scene_enrichment() {
        // The icon FABs paint the "+" glyph; enrichment must NOT overwrite
        // the explicit action names with the glyph.
        let state = idle();
        let scene = pinion_core::Owner::new().run(|| view(&state, &Frame::new()));
        let mut nodes = FabView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        for (node, name) in nodes.iter().zip(NAMES) {
            assert_eq!(node.name.as_deref(), Some(name), "explicit name survives enrichment");
        }
    }

    #[test]
    fn focused_fab_marks_only_that_node() {
        let nodes = FabView::access_node(&idle(), Some(FAB_TAGS[2]));
        assert!(!nodes[0].state.focused);
        assert!(nodes[2].state.focused);
    }

    #[test]
    fn hover_state_sets_the_interaction_flag() {
        let mut state = idle();
        state[0] = ButtonState::Hover;
        let nodes = FabView::access_node(&state, None);
        assert!(nodes[0].state.hovered);
        assert!(!nodes[0].state.pressed);
    }
}
