//! `hello-filter-chip` — R753 §5.38 §5.40 Material 3 **filter chips**: a
//! free-standing bar of independently-toggleable chips (multi-select).
//!
//! This is the **2nd consumer** of the
//! [`pinion_core::widgets::toggle_group`] interaction substrate (the first
//! is `hello-segmented-multi`, R733). A filter-chip set and a multi-select
//! segmented button are the *same interaction class* — N independent
//! [`ToggleExternal`](pinion_core::widgets::toggle::ToggleExternal)s, each
//! its own Tab stop, under a WAI-ARIA `group` parent, lowering to
//! `button[aria-pressed]` — so the keyboard model, the §5.15 introspect
//! reader, the residue-free boot seed and the AccessKit tree builder are
//! shared verbatim through the substrate (`toggle_group::apply_key` /
//! `read_toggle` / `boot_toggle` / `extra_toggles` and
//! [`pinion_a11y::toggle_button_group_nodes`]). Building this binding is
//! what forced that lift: a divergence between two consumers of a standard
//! a11y interaction would be a bug, not a style choice, so the shared core
//! moved out of `hello-segmented-multi` and both now consume it (SSOT — no
//! third copy).
//!
//! **What genuinely diverges is the paint.** A segmented button is a single
//! joined stadium *track* of adjacent pills; filter chips are detached,
//! M3-corner-radius (8 px) rounded rectangles separated by a gap, with an
//! **`Outline` border while unselected** (the defining M3 filter-chip
//! affordance) that drops away when selected. So the chip skin lives inline
//! here (the 1st chip-paint consumer; a 2nd chip consumer — input / assist
//! chips — would trigger a `pinion_widget_paint::chip` lift, the
//! inline-first rule). The hover / pressed / disabled overlay routes
//! through the shared [`state_layer`](pinion_widget_paint::state_layer) SSOT (R752), not raw opacity
//! literals.
//!
//! Palette note: M3 filter chips fill the selected state with a
//! `SecondaryContainer` tone (low emphasis). This gallery's palette has no
//! secondary tier, so the selected fill uses [`ColorRole::Accent`] — the
//! available high-emphasis container — which also reads clearly in the live
//! pixel / XTEST checks. A secondary-tier palette expansion is the
//! canonical follow-up (carry).
//!
//! Keyboard ([`toggle_group::apply_key`], WAI-ARIA APG toggle-button
//! group): each chip is its own Tab stop, so `Tab` / `Shift+Tab` move
//! between chips; `Space` / `Enter` toggle the focused chip; `ArrowRight` /
//! `ArrowLeft` (and `ArrowDown` / `ArrowUp`) rove focus with wrap, `Home` /
//! `End` jump to the first / last chip.

use pinion_a11y::{AccessNode, ToggleSegment, WidgetA11y, toggle_button_group_nodes};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Color, Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::chip::{self, CHIP_HEIGHT, OUTLINE_W};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFilterChipRenderer, HelloFilterChipRendererError);

const WIN_W: u32 = 480;
const WIN_H: u32 = 120;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across the
/// example gallery.
const THEME_TAG: &str = "app";

/// Chip count. Four reads as a realistic M3 "filter bar" and exercises
/// wrap-around arrow roving (last → first).
const N: usize = 4;

/// Per-chip dispatch tags. `&'static str` (not `format!`) because
/// each tag is marked `.with_focusable(true)` in the view fn (the
/// scene-derived §5.39 Tab stops). Each tag
/// lands on one chip, so the input router hit-tests a click on chip `i`
/// straight to that chip's `ToggleExternal`.
const CHIP_TAGS: [&str; N] = ["chip_0", "chip_1", "chip_2", "chip_3"];

/// The WAI-ARIA §3.6 `group` container tag — carried by the (invisible)
/// flex row that holds the chips, so the group's AT bounds attach to the
/// chip strip.
const GROUP_TAG: &str = "chip_group";

/// Chip labels — single source of truth for the painted `TextNode` and the
/// explicit AccessKit `button` name, so the screen-reader text and painted
/// text never diverge. A maps-style filter bar.
const LABELS: [&str; N] = ["Nearby", "Open now", "Top rated", "Offers"];

/// Boot defaults: Nearby + Top rated on, the rest off — a multi-select
/// control has no "exactly one" invariant, and a mixed boot frame gives the
/// `PINION_SCREENSHOT` capture both a selected (Accent) chip and an
/// unselected (outlined) chip to verify in live pixels.
const BOOT_ON: [bool; N] = [true, false, true, false];

/// Filter-chip width — fixed (filter-bar reads as an even strip). Filter-
/// specific, so it stays local; the chip height / radius / inner gap /
/// outline width come from the shared [`pinion_widget_paint::chip`]
/// substrate (R756 lift).
const CHIP_W: u32 = 104;
/// Gap between detached chips (no enclosing track). Filter-specific.
const CHIP_GAP: u32 = 8;
const LABEL_FONT_PX: u32 = 15;
const CHECK_FONT_PX: u32 = 13;
/// U+2713 CHECK MARK — the M3 filter-chip "selected" leading affordance.
/// Named const + escape per the non-ASCII-source rule (raw glyph only in
/// doc strings).
const CHECK_GLYPH: &str = "\u{2713}";

/// Cached projection: one `(ToggleState, on)` pair per chip plus the §5.40
/// AT-side focus index. `Copy` because both inner types are `Copy`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct ChipBarState {
    rows: [(ToggleState, bool); N],
    /// Which chip currently owns shell focus, if any (drives only the AT
    /// `focused` flag — each chip is its own Tab stop, so there is no
    /// roving active-descendant).
    focused: Option<usize>,
}

impl ChipBarState {
    fn idle() -> Self {
        Self {
            rows: [(ToggleState::Idle, false); N],
            focused: None,
        }
    }
}

/// view-fn (§6.3): pure sync `ChipBarState -> Scene`. Builds an invisible
/// flex row (carrying [`GROUP_TAG`]) holding N detached chip rectangles.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ChipBarState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let chips: Vec<Scene> = (0..N)
        .map(|i| chip(i, state.rows[i].0, state.rows[i].1, &theme))
        .collect();
    // The row carries `GROUP_TAG` and *no* fill — the WAI-ARIA `group` is a
    // passive container, and chips are free-standing (no segmented track).
    let row = Scene::Container(
        ContainerNode::new(chips).with_tag(GROUP_TAG).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(CHIP_GAP),
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

/// One chip — selected (Accent fill, leading check, no border) or
/// unselected (transparent fill, `Outline` border), with the shared
/// hover / pressed / disabled [`state_layer`](pinion_widget_paint::state_layer) overlay. Tagged `chip_{index}`
/// so a cursor routes straight to that chip's `ToggleExternal`.
fn chip(index: usize, state: ToggleState, on: bool, theme: &Theme) -> Scene {
    let ink = chip_ink(theme, on, state);
    let mut children: Vec<Scene> = Vec::with_capacity(2);
    if on {
        children.push(Scene::Text(TextNode::styled(
            CHECK_GLYPH,
            Rect::default(),
            TextStyle::new().with_size_px(CHECK_FONT_PX).with_fg(ink),
        )));
    }
    children.push(Scene::Text(TextNode::styled(
        LABELS[index],
        Rect::default(),
        TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(ink),
    )));
    // R756 — chip container skin (state-layer-tinted fill, CHIP_RADIUS,
    // optional outline) + inner layout come from the shared
    // `pinion_widget_paint::chip` substrate; this binding supplies only the
    // base fill (the filter-chip affordance) and the fixed CHIP_W width.
    let style = chip::chip_style(
        chip_fill_base(theme, on),
        chip_border(theme, on),
        state,
        theme,
    );
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(CHIP_TAGS[index])
            .with_style(style)
            // R1020 §5.39 — each chip is a Tab stop; opt its tag-carrying
            // Container into the scene-derived focus enumeration.
            .with_layout(
                chip::chip_layout(Size::px(CHIP_W, CHIP_HEIGHT), None).with_focusable(true),
            ),
    )
}

/// Selected chip resting fill = `Accent` (see palette note in the module
/// docs); unselected = transparent (window surface shows through). The
/// hover / pressed / disabled overlay is applied by [`chip::chip_style`]
/// through the shared R752 [`state_layer`](pinion_widget_paint::state_layer) SSOT (no raw opacity literals
/// here), so this is just the base-color chooser.
fn chip_fill_base(theme: &Theme, on: bool) -> Color {
    if on {
        theme.resolve(ColorRole::Accent)
    } else {
        Color::rgba(0, 0, 0, 0)
    }
}

/// Unselected chips carry the M3 filter-chip `Outline` border; selected
/// chips drop it (the tonal fill is the affordance).
fn chip_border(theme: &Theme, on: bool) -> Option<Border> {
    if on {
        None
    } else {
        Some(Border::new(theme.resolve(ColorRole::Outline), OUTLINE_W))
    }
}

/// Label + check ink: `OnAccent` on a selected chip, the muted on-surface
/// tone on an unselected (or disabled) chip.
fn chip_ink(theme: &Theme, on: bool, state: ToggleState) -> Color {
    if matches!(state, ToggleState::Disabled) {
        return theme.resolve(ColorRole::OnSurfaceMuted);
    }
    if on {
        theme.resolve(ColorRole::OnAccent)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    }
}

struct FilterChipView;

impl WidgetCore for FilterChipView {
    type State = ChipBarState;
    // Every state change flows through `apply_key` (keyboard) or the
    // InputRouter's per-chip pointer dispatch (pointer), never the shell's
    // enum-typed `keybinding` channel — so `()` satisfies the `Copy` bound.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(toggle_group::boot_toggle(BOOT_ON[0]))
    }

    /// Chips 1..N as sibling externals; chip 0 is the primary
    /// [`Self::create_external`]. The substrate wraps the root as
    /// `Scene::Container([primary, ...extras])`, and the input router's
    /// depth-first walk dispatches each chip click by tag.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        toggle_group::extra_toggles(&CHIP_TAGS, &BOOT_ON)
    }

    fn tag() -> &'static str {
        CHIP_TAGS[0]
    }

    fn read_state(scene: &Scene) -> ChipBarState {
        let mut out = ChipBarState::idle();
        for (i, slot) in out.rows.iter_mut().enumerate() {
            *slot = toggle_group::read_toggle(scene, CHIP_TAGS[i]);
        }
        out
    }

    fn view(state: ChipBarState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // Keys flow through `apply_key` directly; the enum-typed channel
        // stays unused (mirrors hello-segmented-multi).
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-filter-chip (R753 §5.38 Material 3 filter chips)"
    }

    /// WAI-ARIA toggle-button-group keyboard model — delegated wholesale to
    /// the shared [`toggle_group::apply_key`] substrate.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        toggle_group::apply_key(scene, focused, key, &CHIP_TAGS)
    }

    fn fmt_state_log(state: &ChipBarState) -> String {
        let rows = state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (s, on))| format!("{i}={}{}", s.as_name(), if *on { "+" } else { "-" }))
            .collect::<Vec<_>>()
            .join(" ");
        match state.focused {
            Some(idx) => format!("{rows} focused={idx}"),
            None => rows,
        }
    }
}

impl WidgetA11y for FilterChipView {
    /// R753 §5.40 — one [`AriaRole::Group`](pinion_a11y::AriaRole::Group)
    /// parent (`"Filters"`) + one
    /// [`AriaRole::Button`](pinion_a11y::AriaRole::Button) per chip carrying
    /// **`aria-pressed`**, built by the shared [`toggle_button_group_nodes`]
    /// substrate from this binding's `(state, on)` rows and [`LABELS`].
    fn access_node(state: &ChipBarState, focused: Option<&str>) -> Vec<AccessNode> {
        let segments: Vec<ToggleSegment<'_>> = (0..N)
            .map(|i| ToggleSegment {
                tag: CHIP_TAGS[i],
                label: LABELS[i],
                state: state.rows[i].0,
                on: state.rows[i].1,
            })
            .collect();
        toggle_button_group_nodes(GROUP_TAG, "Filters", &segments, focused)
    }
}

impl WidgetView for FilterChipView {
    type Renderer = HelloFilterChipRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<FilterChipView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(on: [bool; N]) -> ChipBarState {
        let mut s = ChipBarState::idle();
        for (i, slot) in s.rows.iter_mut().enumerate() {
            slot.1 = on[i];
        }
        s
    }

    // ── view / composition (binding-specific chip paint) ──────────────────

    #[test]
    fn view_carries_primary_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<FilterChipView>(
            ChipBarState::idle(),
            &Frame::new(),
        );
    }

    #[test]
    fn view_carries_every_chip_tag_and_the_group_row() {
        let scene = pinion_core::Owner::new().run(|| view(state_with(BOOT_ON), &Frame::new()));
        assert!(scene.contains_tag(GROUP_TAG), "row carries the group tag");
        for tag in CHIP_TAGS {
            assert!(scene.contains_tag(tag), "chip tag {tag} present");
        }
    }

    /// Recursively find the chip container tagged `tag`.
    fn find_chip<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(c);
                }
                c.children.iter().find_map(|ch| find_chip(ch, tag))
            }
            _ => None,
        }
    }

    #[test]
    fn selected_chip_is_filled_with_no_border_unselected_is_outlined() {
        let scene = pinion_core::Owner::new()
            .run(|| view(state_with([true, false, false, false]), &Frame::new()));
        let sel = find_chip(&scene, CHIP_TAGS[0]).expect("chip 0");
        let unsel = find_chip(&scene, CHIP_TAGS[1]).expect("chip 1");
        // Selected: opaque Accent fill, no border.
        assert_eq!(sel.style.fill.a, 255, "selected chip is opaque");
        assert!(
            sel.style.border.is_none(),
            "selected chip drops the outline"
        );
        // Unselected: transparent fill, an Outline border.
        assert_eq!(unsel.style.fill.a, 0, "unselected chip is transparent");
        assert!(unsel.style.border.is_some(), "unselected chip is outlined");
    }

    /// Count a chip container's direct `Text` children (selected = check
    /// glyph + label = 2; unselected = label only = 1).
    fn text_child_count(scene: &Scene, tag: &str) -> usize {
        find_chip(scene, tag).map_or(0, |c| {
            c.children
                .iter()
                .filter(|ch| matches!(ch, Scene::Text(_)))
                .count()
        })
    }

    #[test]
    fn selected_chip_has_check_glyph_plus_label() {
        let scene = pinion_core::Owner::new()
            .run(|| view(state_with([true, false, false, false]), &Frame::new()));
        assert_eq!(
            text_child_count(&scene, CHIP_TAGS[0]),
            2,
            "selected: check + label"
        );
        assert_eq!(
            text_child_count(&scene, CHIP_TAGS[1]),
            1,
            "unselected: label only"
        );
    }

    // ── WidgetCore wiring (delegation smoke — mechanics live in the
    //    toggle_group substrate's own tests) ───────────────────────────────

    #[test]
    fn every_chip_is_a_tab_stop() {
        // §5.39: tree order IS tab order — collected from the paint scene.
        let scene = pinion_core::Owner::new().run(|| view(state_with(BOOT_ON), &Frame::new()));
        assert_eq!(
            scene.collect_focusable_tags(),
            CHIP_TAGS
                .iter()
                .map(|t| (*t).to_owned())
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn apply_key_space_toggles_focused_chip_through_wiring() {
        let mut scene = scene_fixture();
        // Chip 1 boots Off; Space (via the WidgetCore method) turns it On
        // without touching siblings — proving the substrate is wired in.
        assert!(FilterChipView::apply_key(
            &mut scene,
            Some(CHIP_TAGS[1]),
            "Space",
            pinion_core::Modifiers::empty(),
        ));
        let s = FilterChipView::read_state(&scene);
        assert!(s.rows[1].1, "focused chip 1 toggled On");
        assert_eq!(s.rows[0].1, BOOT_ON[0], "chip 0 untouched");
        assert_eq!(s.rows[2].1, BOOT_ON[2], "chip 2 untouched");
    }

    // ── a11y (delegation + binding labels / scene enrichment) ─────────────

    #[test]
    fn emits_group_parent_plus_n_aria_pressed_buttons() {
        let nodes = FilterChipView::access_node(&state_with(BOOT_ON), None);
        assert_eq!(nodes.len(), N + 1, "one group + N chips");
        assert_eq!(
            nodes[0].role,
            pinion_a11y::AriaRole::Group,
            "first node is the group"
        );
        assert_eq!(nodes[0].name.as_deref(), Some("Filters"));
        assert_eq!(nodes[0].children.len(), N, "group references every chip");
        for (i, node) in nodes[1..].iter().enumerate() {
            assert_eq!(node.role, pinion_a11y::AriaRole::Button, "chip is a button");
            assert_eq!(
                node.state.checked,
                Some(BOOT_ON[i]),
                "aria-pressed mirrors on/off"
            );
            assert_eq!(node.name.as_deref(), Some(LABELS[i]));
        }
    }

    #[test]
    fn names_survive_scene_enrichment() {
        let state = state_with(BOOT_ON);
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = FilterChipView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        for (i, node) in nodes[1..].iter().enumerate() {
            assert_eq!(node.name.as_deref(), Some(LABELS[i]), "explicit label kept");
        }
    }

    /// Build a fresh scene mirroring the shell's boot composition
    /// (`Scene::Container([primary, ...extras])`) so `apply_key` /
    /// `read_state` walk the live topology.
    fn scene_fixture() -> Scene {
        use pinion_core::scene::ExternalNode;
        let primary = Scene::External(
            ExternalNode::new(FilterChipView::create_external()).with_tag(CHIP_TAGS[0]),
        );
        let mut children = vec![primary];
        for extra in FilterChipView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag.into_owned()),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
