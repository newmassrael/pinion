//! `hello-segmented-multi` — R733 §5.38 §5.40 **multi-select** Material
//! 3 segmented button (a toggle-button group).
//!
//! Where `hello-segmented-button` (R728) is *single-select* — a
//! [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal)
//! with at most one segment chosen — this binding is *multi-select*: any
//! subset of segments may be on at once, and each segment toggles
//! independently. The interaction substrate is therefore **not** a radio
//! group at all; it is N independent
//! [`ToggleExternal`](pinion_core::widgets::toggle::ToggleExternal)s
//! composed through the R55.D.5
//! [`WidgetCore::create_extra_externals`] slot — the same
//! homogeneous-cluster path `examples/hello-accordion` (N disclosures)
//! and `examples/settings-panel` (six checkboxes) use, with zero new
//! coordinator substrate.
//!
//! **New reusable a11y primitive (R733): `aria-pressed`.** A multi-select
//! segment is a *toggle button*: it lowers to AccessKit
//! [`AriaRole::Button`] carrying
//! [`AccessState::checked`](pinion_a11y::AccessState::checked) =
//! `Some(on)`. A `button` role with a toggled state reflects
//! **`aria-pressed`** ("pressed" / "not pressed"), distinct from the
//! `aria-checked` a checkbox / switch / radio reflects — even though both
//! lower through the same AccessKit `set_toggled` call (the split is a
//! function of the role, exactly as WAI-ARIA defines). The segments sit
//! under a new [`AriaRole::Group`] parent (WAI-ARIA §3.6 `group`), a
//! labelled passive container — distinct from `RadioGroup` (single-select
//! `radio`/`aria-checked` children) and `Toolbar` (roving-tabindex). This
//! is the framework's first `aria-pressed` consumer; toolbars and
//! text-formatting bars (Bold / Italic / Underline) reuse the same
//! `button[aria-pressed]` pattern.
//!
//! Skin: the tonal **track** + per-segment pills mirror the R728 single-
//! select paint (an `Accent` pill + leading check glyph while pressed,
//! transparent otherwise, with the shared hover / pressed state-layer
//! overlay). This is the **2nd identical consumer** of the *opinionated*
//! segmented paint (typed over `ToggleState` instead of `RadioState`); per
//! the R703 rule the opinionated paint stays an honest deferred carry
//! until a 3rd **identical** consumer triggers a
//! `pinion_widget_paint::segmented` lift. What is genuinely new is the
//! multi-select *interaction* (independent toggles) + the `aria-pressed`
//! a11y mapping.
//!
//! Hit-target: each segment is tagged `seg_multi_{i}` (a *whole* tag, not
//! the R51.42 `tag#i` composite sub-index R728 uses — each segment is its
//! own [`External`], so a cursor on segment `i` routes straight to that
//! `ToggleExternal`'s pointer arc). AI clients reach the same surface via
//! `query "/seg_multi_{i}/external/value"`, the AccessKit tree
//! (`group` + per-segment `button` with `aria-pressed`), and the identical
//! `invoke "send"` wire the keyboard path uses (§2 #2).
//!
//! Keyboard (`apply_key`, WAI-ARIA APG toggle-button group, mirroring the
//! accordion): each segment is its **own Tab stop**
//! ([`focusable_tags`] enumerates all three), so `Tab` / `Shift+Tab` move
//! between segments; `Space` / `Enter` toggle the focused segment;
//! `ArrowRight` / `ArrowLeft` (and `ArrowDown` / `ArrowUp`) move focus one
//! step with wrap, `Home` / `End` jump to first / last — the focus moves
//! funnel through the R664 [`pinion_core::focus_request`] mailbox.

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{Color, Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSegmentedMultiRenderer, HelloSegmentedMultiRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 160;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across the
/// example gallery.
const THEME_TAG: &str = "app";

/// Segment count. Three is enough to exercise wrap-around arrow roving
/// (last → first) and to read as a real media-filter toggle bar.
const N: usize = 3;

/// Per-segment dispatch tags. `&'static str` (not `format!`) because
/// [`WidgetCore::focusable_tags`] returns `Vec<&'static str>` — the same
/// fixed-cluster convention `hello-accordion` uses. Each tag lands on one
/// segment pill, so the input router hit-tests a click on segment `i`
/// straight to that segment's `ToggleExternal`.
const SEGMENT_TAGS: [&str; N] = ["seg_multi_0", "seg_multi_1", "seg_multi_2"];

/// The WAI-ARIA §3.6 `group` container tag — doubles as the track's paint
/// tag so the group's AT bounds attach to the painted track strip.
const GROUP_TAG: &str = "seg_multi_group";

/// Segment labels — single source of truth for both the painted
/// `TextNode` and the explicit AccessKit `button` name, so the
/// screen-reader text and painted text never diverge.
const LABELS: [&str; N] = ["Photos", "Videos", "Audio"];

/// Boot defaults: Photos + Videos on, Audio off. A multi-select control
/// has no "exactly one" invariant, so seeding two pressed segments is a
/// legitimate default — and (per the R728 lesson) gives the boot
/// `PINION_SCREENSHOT` frame both a pressed pill *and* an unpressed
/// segment to verify in live pixels.
const BOOT_ON: [bool; N] = [true, true, false];

const SEG_W: u32 = 110;
const SEG_H: u32 = 40;
/// Track inset so each pressed pill floats inside the stadium track.
const TRACK_PAD: u32 = 4;
/// Stadium track radius = half-height + inset (fully rounded ends).
const TRACK_RADIUS: u32 = SEG_H / 2 + TRACK_PAD;
/// Pressed pill radius = half-height (fully rounded).
const PILL_RADIUS: u32 = SEG_H / 2;
const LABEL_FONT_PX: u32 = 16;
const CHECK_FONT_PX: u32 = 14;
const CHECK_GAP: u32 = 6;
/// U+2713 CHECK MARK — the M3 segmented "selected" affordance (shown on
/// every pressed segment in multi-select). Named const + escape per the
/// non-ASCII-source rule (raw glyph only in doc strings).
const CHECK_GLYPH: &str = "\u{2713}";

/// Cached projection: one `(ToggleState, on)` pair per segment plus the
/// §5.40 AT-side roving-focus index. `Copy` because both inner types are
/// `Copy`, so the shell hands the snapshot into the paint closure without
/// lifetime gymnastics.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct GroupState {
    rows: [(ToggleState, bool); N],
    /// Which segment currently owns shell focus, if any (drives only the
    /// AT `focused` flag — each segment is its own Tab stop, so there is
    /// no roving active-descendant).
    focused: Option<usize>,
}

impl GroupState {
    fn idle() -> Self {
        Self {
            rows: [(ToggleState::Idle, false); N],
            focused: None,
        }
    }
}

/// view-fn (§6.3): pure sync `GroupState -> Scene`. Builds the tonal track
/// row holding N independent segment pills, each tagged `seg_multi_{i}`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: GroupState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let segments: Vec<Scene> = (0..N)
        .map(|i| segment(i, state.rows[i].0, state.rows[i].1, &theme))
        .collect();
    // The track carries `GROUP_TAG` so the WAI-ARIA `group`'s AT bounds
    // attach to the painted track strip (R55.G.18 §5.49).
    let track = Scene::Container(
        ContainerNode::new(segments)
            .with_tag(GROUP_TAG)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                    .with_corner_radius(TRACK_RADIUS),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_padding(Rect::new(TRACK_PAD, TRACK_PAD, TRACK_PAD, TRACK_PAD))
                    .with_gap(0),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![track])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// One segment — pressed fill + (when pressed) leading check glyph +
/// label. Tagged `seg_multi_{index}` so a cursor routes straight to that
/// segment's [`ToggleExternal`].
///
/// R733 carry: this is the 2nd *identical* consumer of the R728 segmented
/// paint (typed over `ToggleState` rather than `RadioState`); a 3rd
/// identical consumer triggers the `pinion_widget_paint::segmented` lift.
fn segment(index: usize, state: ToggleState, on: bool, theme: &Theme) -> Scene {
    let label_color = segment_label_color(theme, on, state);
    let mut children: Vec<Scene> = Vec::with_capacity(2);
    if on {
        children.push(Scene::Text(TextNode::styled(
            CHECK_GLYPH,
            Rect::default(),
            TextStyle::new().with_size_px(CHECK_FONT_PX).with_fg(label_color),
        )));
    }
    children.push(Scene::Text(TextNode::styled(
        LABELS[index],
        Rect::default(),
        TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(label_color),
    )));
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(SEGMENT_TAGS[index])
            .with_style(
                BoxStyle::filled(segment_fill(theme, state, on))
                    .with_corner_radius(PILL_RADIUS),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(CHECK_GAP)
                    .with_size(Size::px(SEG_W, SEG_H)),
            ),
    )
}

/// Pressed pill fill (`Accent`) or transparent (track shows through), with
/// the hover / pressed state-layer overlay shared across the widget
/// gallery.
fn segment_fill(theme: &Theme, state: ToggleState, on: bool) -> Color {
    let base = if on {
        theme.resolve(ColorRole::Accent)
    } else {
        Color::rgba(0, 0, 0, 0)
    };
    match state {
        ToggleState::Idle | ToggleState::Disabled => base,
        ToggleState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
        ToggleState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
    }
}

fn segment_label_color(theme: &Theme, on: bool, state: ToggleState) -> Color {
    if matches!(state, ToggleState::Disabled) {
        return theme.resolve(ColorRole::OnSurfaceMuted);
    }
    if on {
        theme.resolve(ColorRole::OnAccent)
    } else {
        theme.resolve(ColorRole::OnSurface)
    }
}

/// Read one segment's `(state, on)` from the live scene via the §5.15
/// introspect channel — the same path an RPC
/// `scene/query /seg_multi_{i}/external/value` request walks, so the
/// cached projection and the AI client never diverge.
fn read_segment(scene: &Scene, tag: &str) -> (ToggleState, bool) {
    let Some(node) = scene.find_external_with_tag(tag) else {
        return (ToggleState::Idle, false);
    };
    let Some(intro) = node.handle.introspect() else {
        return (ToggleState::Idle, false);
    };
    let state = match intro.query("state") {
        Some(IntrospectValue::Text(name)) => ToggleState::from_name_or_default(&name),
        _ => ToggleState::Idle,
    };
    let on = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
    (state, on)
}

/// Construct a [`ToggleExternal`] seeded to `on` without leaving a hover /
/// pressed residue: `KeyboardActivate` flips the value Off → On while the
/// interaction state stays `Idle`, so the boot frame paints a clean Accent
/// pill (the R728 boot-seed lesson).
fn boot_toggle(on: bool) -> ToggleExternal {
    let mut ext = ToggleExternal::new();
    if on {
        ext.send(ToggleEvent::KeyboardActivate);
    }
    ext
}

struct SegmentedMultiView;

impl WidgetCore for SegmentedMultiView {
    type State = GroupState;
    // Every state change flows through `apply_key` (keyboard) or the
    // InputRouter's per-segment pointer dispatch (pointer), never the
    // shell's enum-typed `keybinding` channel — so `()` satisfies the
    // trait's `Copy` bound (mirror of `hello-accordion`).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(boot_toggle(BOOT_ON[0]))
    }

    /// Segments 1..N as sibling externals; segment 0 is the primary
    /// [`Self::create_external`]. The substrate wraps the root as
    /// `Scene::Container([primary, ...extras])`, and the input router's
    /// depth-first walk dispatches each pill click by tag.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        (1..N)
            .map(|i| ExtraExternal::new(SEGMENT_TAGS[i], Box::new(boot_toggle(BOOT_ON[i]))))
            .collect()
    }

    fn tag() -> &'static str {
        SEGMENT_TAGS[0]
    }

    fn read_state(scene: &Scene) -> GroupState {
        let mut out = GroupState::idle();
        for (i, slot) in out.rows.iter_mut().enumerate() {
            *slot = read_segment(scene, SEGMENT_TAGS[i]);
        }
        out
    }

    fn view(state: GroupState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // Keys flow through `apply_key` directly; the enum-typed channel
        // stays unused (mirrors hello-accordion / hello-radio-group).
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-segmented-multi (R733 §5.38 multi-select segmented)"
    }

    /// Each segment is its own Tab stop (WAI-ARIA toggle-button group:
    /// every toggle button is in the document focus order), unlike the
    /// single-select segmented control's single-tab-stop roving model.
    fn focusable_tags() -> Vec<&'static str> {
        SEGMENT_TAGS.to_vec()
    }

    /// WAI-ARIA toggle-button-group keyboard model, gated on the focused
    /// segment (roving-tabindex `apply_key` discipline — keys route only
    /// when one of our segments owns focus):
    ///
    /// - `Space` / `Enter` — toggle the focused segment (independent, no
    ///   mutual exclusion);
    /// - `ArrowRight` / `ArrowLeft` (and `ArrowDown` / `ArrowUp`) — move
    ///   focus to the next / previous segment, wrapping at the ends;
    /// - `Home` / `End` — move focus to the first / last segment.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let Some(focused_tag) = focused else {
            return false;
        };
        let Some(idx) = SEGMENT_TAGS.iter().position(|t| *t == focused_tag) else {
            return false;
        };
        match key {
            "Space" | "Enter" => {
                let Some(node) = scene.find_external_with_tag_mut(SEGMENT_TAGS[idx]) else {
                    return false;
                };
                let Some(intro) = node.handle.introspect_mut() else {
                    return false;
                };
                intro
                    .invoke("send", IntrospectValue::Text("KeyboardActivate".to_string()))
                    .is_ok()
            }
            "ArrowRight" | "ArrowDown" => {
                pinion_core::focus_request::request(SEGMENT_TAGS[(idx + 1) % N]);
                true
            }
            "ArrowLeft" | "ArrowUp" => {
                pinion_core::focus_request::request(SEGMENT_TAGS[(idx + N - 1) % N]);
                true
            }
            "Home" => {
                pinion_core::focus_request::request(SEGMENT_TAGS[0]);
                true
            }
            "End" => {
                pinion_core::focus_request::request(SEGMENT_TAGS[N - 1]);
                true
            }
            _ => false,
        }
    }

    fn fmt_state_log(state: &GroupState) -> String {
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

impl WidgetA11y for SegmentedMultiView {
    /// R733 §5.40 — one [`AriaRole::Group`] parent (`"Show"`) + one
    /// [`AriaRole::Button`] per segment carrying **`aria-pressed`**
    /// ([`AccessState::checked`] = `Some(on)` on a `button` role). Each
    /// segment carries an **explicit** name from [`LABELS`] (explicit
    /// names survive `enrich_names_from_scene`, so the leading check
    /// glyph's `TextNode` cannot corrupt the accessible name) and
    /// `focused` when that segment owns shell focus.
    fn access_node(state: &GroupState, focused: Option<&str>) -> Vec<AccessNode> {
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(N + 1);
        let mut group = AccessNode::new(GROUP_TAG, AriaRole::Group).with_name("Show");
        for tag in SEGMENT_TAGS {
            group = group.with_child(tag);
        }
        nodes.push(group);
        for i in 0..N {
            let (interaction, on) = state.rows[i];
            nodes.push(
                AccessNode::new(SEGMENT_TAGS[i], AriaRole::Button)
                    .with_name(LABELS[i])
                    .with_state(AccessState {
                        focused: focused == Some(SEGMENT_TAGS[i]),
                        disabled: matches!(interaction, ToggleState::Disabled),
                        hovered: matches!(interaction, ToggleState::Hover),
                        pressed: matches!(interaction, ToggleState::Pressed),
                        // R733 — `Some(on)` on a `button` role = aria-pressed.
                        checked: Some(on),
                    }),
            );
        }
        nodes
    }
}

impl WidgetView for SegmentedMultiView {
    type Renderer = HelloSegmentedMultiRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<SegmentedMultiView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(on: [bool; N]) -> GroupState {
        let mut s = GroupState::idle();
        for (i, slot) in s.rows.iter_mut().enumerate() {
            slot.1 = on[i];
        }
        s
    }

    // ── view / composition ────────────────────────────────────────────

    #[test]
    fn view_carries_primary_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<SegmentedMultiView>(
            GroupState::idle(),
            &Frame::new(),
        );
    }

    #[test]
    fn view_carries_every_segment_tag_and_the_group_track() {
        let scene =
            pinion_core::Owner::new().run(|| view(state_with(BOOT_ON), &Frame::new()));
        assert!(scene.contains_tag(GROUP_TAG), "track carries the group tag");
        for tag in SEGMENT_TAGS {
            assert!(scene.contains_tag(tag), "segment tag {tag} present");
        }
    }

    /// Recursively find the container tagged `tag` and count its direct
    /// `Text` children (pressed segment = check glyph + label = 2;
    /// unpressed = label only = 1).
    fn text_child_count(scene: &Scene, tag: &str) -> Option<usize> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(
                        c.children.iter().filter(|ch| matches!(ch, Scene::Text(_))).count(),
                    );
                }
                c.children.iter().find_map(|ch| text_child_count(ch, tag))
            }
            _ => None,
        }
    }

    #[test]
    fn pressed_segment_has_glyph_plus_label_unpressed_label_only() {
        let scene = pinion_core::Owner::new()
            .run(|| view(state_with([true, false, false]), &Frame::new()));
        assert_eq!(
            text_child_count(&scene, SEGMENT_TAGS[0]),
            Some(2),
            "pressed: check glyph + label",
        );
        assert_eq!(
            text_child_count(&scene, SEGMENT_TAGS[1]),
            Some(1),
            "unpressed: label only",
        );
    }

    // ── focus / keyboard model ─────────────────────────────────────────

    #[test]
    fn every_segment_is_a_tab_stop() {
        assert_eq!(SegmentedMultiView::focusable_tags(), SEGMENT_TAGS.to_vec());
    }

    #[test]
    fn space_toggles_only_the_focused_segment() {
        let mut scene = scene_fixture();
        // Segment 2 boots Off; Space turns it On without touching siblings.
        assert!(SegmentedMultiView::apply_key(
            &mut scene,
            Some(SEGMENT_TAGS[2]),
            "Space",
            pinion_core::Modifiers::empty(),
        ));
        let s = SegmentedMultiView::read_state(&scene);
        assert!(s.rows[2].1, "focused segment 2 toggled On");
        assert_eq!(s.rows[0].1, BOOT_ON[0], "segment 0 untouched");
        assert_eq!(s.rows[1].1, BOOT_ON[1], "segment 1 untouched");
    }

    #[test]
    fn enter_also_toggles_and_is_independent_not_exclusive() {
        let mut scene = scene_fixture();
        // Segment 0 boots On; Enter turns it Off — and segment 1 (also On
        // at boot) stays On, proving multi-select independence (no radio
        // mutual exclusion).
        assert!(SegmentedMultiView::apply_key(
            &mut scene,
            Some(SEGMENT_TAGS[0]),
            "Enter",
            pinion_core::Modifiers::empty(),
        ));
        let s = SegmentedMultiView::read_state(&scene);
        assert!(!s.rows[0].1, "Enter toggled segment 0 Off");
        assert!(s.rows[1].1, "segment 1 stays On — not mutually exclusive");
    }

    #[test]
    fn arrow_right_requests_next_segment_wrapping() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(SegmentedMultiView::apply_key(
            &mut scene,
            Some(SEGMENT_TAGS[N - 1]),
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(
            pinion_core::focus_request::drain().as_deref(),
            Some(SEGMENT_TAGS[0]),
            "wraps last -> first",
        );
    }

    #[test]
    fn arrow_left_requests_previous_segment_wrapping() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(SegmentedMultiView::apply_key(
            &mut scene,
            Some(SEGMENT_TAGS[0]),
            "ArrowLeft",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(
            pinion_core::focus_request::drain().as_deref(),
            Some(SEGMENT_TAGS[N - 1]),
            "wraps first -> last",
        );
    }

    #[test]
    fn home_and_end_jump_to_first_and_last() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(SegmentedMultiView::apply_key(
            &mut scene,
            Some(SEGMENT_TAGS[1]),
            "Home",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(SEGMENT_TAGS[0]));
        assert!(SegmentedMultiView::apply_key(
            &mut scene,
            Some(SEGMENT_TAGS[1]),
            "End",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(SEGMENT_TAGS[N - 1]));
    }

    #[test]
    fn keys_ignored_when_no_segment_focused() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(!SegmentedMultiView::apply_key(
            &mut scene,
            None,
            "Space",
            pinion_core::Modifiers::empty(),
        ));
        assert!(!SegmentedMultiView::apply_key(
            &mut scene,
            Some("some_sibling"),
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain(), None, "no focus request emitted");
    }

    // ── a11y ───────────────────────────────────────────────────────────

    #[test]
    fn emits_group_parent_plus_n_aria_pressed_buttons() {
        let nodes = SegmentedMultiView::access_node(&state_with(BOOT_ON), None);
        assert_eq!(nodes.len(), N + 1, "one group + N segments");
        assert_eq!(nodes[0].role, AriaRole::Group, "first node is the group");
        assert_eq!(nodes[0].name.as_deref(), Some("Show"));
        assert_eq!(nodes[0].children.len(), N, "group references every segment");
        for (i, node) in nodes[1..].iter().enumerate() {
            assert_eq!(node.role, AriaRole::Button, "segment is a button (aria-pressed)");
            assert_eq!(node.state.checked, Some(BOOT_ON[i]), "aria-pressed mirrors on/off");
            assert_eq!(node.name.as_deref(), Some(LABELS[i]));
        }
    }

    #[test]
    fn focused_segment_marks_only_that_node_focused() {
        let nodes = SegmentedMultiView::access_node(&GroupState::idle(), Some(SEGMENT_TAGS[1]));
        // nodes[0] is the group; segments are nodes[1..].
        assert!(!nodes[1].state.focused, "segment 0 not focused");
        assert!(nodes[2].state.focused, "segment 1 focused");
        assert!(!nodes[3].state.focused, "segment 2 not focused");
    }

    #[test]
    fn names_survive_scene_enrichment() {
        let state = state_with(BOOT_ON);
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = SegmentedMultiView::access_node(&state, None);
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
            ExternalNode::new(SegmentedMultiView::create_external()).with_tag(SEGMENT_TAGS[0]),
        );
        let mut children = vec![primary];
        for extra in SegmentedMultiView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag.into_owned()),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
