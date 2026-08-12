//! `hello-segmented-button` — R728 §5.38 §5.40 single-select Material
//! 3 segmented button. Pure **composition** over the R51.44
//! [`RadioGroupExternal`] coordinator: a segmented control *is* a
//! single-select radio group with a different skin, so this binding
//! reuses the entire framework-owned machinery (mutual exclusion,
//! roving tabindex, AccessKit tree) and adds **zero new substrate** —
//! the R697 accordion / R714 combobox composition pattern.
//!
//! Skin: a tonal **track** (`SurfaceContainerHighest`, rounded
//! stadium) holding N contiguous (gap-0) segments. The selected
//! segment is an `Accent`-filled rounded pill carrying a leading check
//! glyph (the M3 single-select "selected" affordance) + `OnAccent`
//! label; unselected segments are transparent (the track shows
//! through) + `OnSurface` label. Hover / Pressed layer a state overlay
//! (`OnSurface` at 0.08 / 0.12) exactly as `hello-radio-group` and the
//! todomvc filter row do.
//!
//! This is the **2nd standalone consumer** of the segmented re-skin
//! pattern; the todomvc filter row (R659/R660 `build_filter_row`) is
//! the 1st. The two diverge in *paint* (this = tonal-track + check
//! glyph, the iOS / M3-expressive lineage; todomvc = M3-outlined
//! gapped buttons, high-emphasis Accent) — so per the R703 rule the
//! *opinionated* segmented paint stays an honest deferred carry until a
//! 3rd **identical** consumer triggers a shared segmented-paint
//! lift. What is genuinely shared (and reused, not duplicated) is the
//! `RadioGroupExternal` coordinator.
//!
//! Hit-target: each segment is tagged `"view_mode#<i>"` so the
//! `InputRouter` R51.42 sub-index split routes a cursor on segment `i`
//! to `invoke("send", Text("<i>:<EventName>"))` against the single
//! composite handle. AI clients reach the same surface through
//! `query("selected_index")`, the per-segment `query("state.<i>")` /
//! `query("selected.<i>")` paths, the AccessKit tree (`radiogroup` +
//! `radio` with `aria-checked` / `aria-activedescendant`), and the
//! identical `invoke "send"` wire the keyboard path uses (§2 #2: RPC
//! headless / human cursor / keyboard all converge on one statechart).
//!
//! Keyboard (`apply_key`, R51.37 / R51.57 §5.39): the group is a
//! single tab stop; when focused, `ArrowRight` / `ArrowLeft` (and
//! `ArrowDown` / `ArrowUp`) move the selection one step with wrap,
//! `Home` / `End` jump to the first / last segment, and the
//! single-key shortcuts `d` / `w` / `m` select Day / Week / Month.

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, RadioCell, WidgetA11y, radiogroup_radio_nodes,
};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::radio::{RadioEvent, RadioState};
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::radio_composite as rc;

use pinion_widget_paint::state_layer::{HOVER, PRESSED};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(
    HelloSegmentedButtonRenderer,
    HelloSegmentedButtonRendererError
);

const WIN_W: u32 = 420;
const WIN_H: u32 = 140;
/// [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key — matches the `"app"` convention shared
/// across the example gallery.
const THEME_TAG: &str = "app";
const N: usize = 3;
const PRIMARY_TAG: &str = "view_mode";

const SEG_W: u32 = 110;
const SEG_H: u32 = 40;
/// Track inset so the selected pill floats inside the stadium track.
const TRACK_PAD: u32 = 4;
/// Stadium track radius = half-height + inset (fully rounded ends).
const TRACK_RADIUS: u32 = SEG_H / 2 + TRACK_PAD;
/// Selected pill radius = half-height (fully rounded).
const PILL_RADIUS: u32 = SEG_H / 2;
const LABEL_FONT_PX: u32 = 16;
const CHECK_FONT_PX: u32 = 14;
const CHECK_GAP: u32 = 6;
/// U+2713 CHECK MARK — the M3 single-select segmented "selected"
/// affordance. Named const + escape per the non-ASCII-source rule
/// (raw glyph only in doc strings).
const CHECK_GLYPH: &str = "\u{2713}";

/// Cached projection of the group. One `(RadioState, selected)` pair
/// per segment plus the §5.40 AT-side active-descendant index. `Copy`
/// because both fields' inner types are `Copy`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct GroupState {
    rows: [(RadioState, bool); N],
    focused: Option<usize>,
}

impl GroupState {
    fn idle() -> Self {
        Self {
            rows: [(RadioState::Idle, false); N],
            focused: None,
        }
    }
}

/// view-fn (§6.3): pure sync `GroupState -> Scene`. Builds the tonal
/// track row holding N contiguous segments, each tagged
/// `"view_mode#<i>"` for the R51.42 sub-index routing.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: GroupState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let segments: Vec<Scene> = (0..N)
        .map(|i| segment(i, state.rows[i].0, state.rows[i].1, &theme))
        .collect();
    // The track carries `PRIMARY_TAG` so `{path: "view_mode"}` AI-side
    // routing and `rect_for_tag` AT bounds attach to the segmented
    // control rather than the full window (R55.G.18 §5.49). The
    // composite `RadioGroupExternal` lives in the *state* scene; this
    // paint container mirrors the tag.
    let track = Scene::Container(
        ContainerNode::new(segments)
            .with_tag(PRIMARY_TAG)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                    .with_corner_radius(TRACK_RADIUS),
            )
            // (R1030 §5.39) hand-composed focus stop — composing view owns the opt-in.
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_focusable(true)
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

/// One segment — selected fill + (when selected) leading check glyph +
/// label. Tagged `"view_mode#<i>"` for R51.42 sub-index dispatch.
fn segment(index: usize, state: RadioState, selected: bool, theme: &Theme) -> Scene {
    let label_color = segment_label_color(theme, selected, state);
    let mut children: Vec<Scene> = Vec::with_capacity(2);
    if selected {
        children.push(Scene::Text(TextNode::styled(
            CHECK_GLYPH,
            Rect::default(),
            TextStyle::new()
                .with_size_px(CHECK_FONT_PX)
                .with_fg(label_color),
        )));
    }
    children.push(Scene::Text(TextNode::styled(
        segment_label(index),
        Rect::default(),
        TextStyle::new()
            .with_size_px(LABEL_FONT_PX)
            .with_fg(label_color),
    )));
    let seg_tag = format!("{PRIMARY_TAG}#{index}");
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(seg_tag)
            .with_style(
                BoxStyle::filled(segment_fill(theme, state, selected))
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

/// Selected pill fill (`Accent`) or transparent (track shows through),
/// with the hover / pressed state-layer overlay shared across the
/// pinion widget gallery.
fn segment_fill(theme: &Theme, state: RadioState, selected: bool) -> Color {
    let base = if selected {
        theme.resolve(ColorRole::Accent)
    } else {
        Color::rgba(0, 0, 0, 0)
    };
    match state {
        RadioState::Idle | RadioState::Disabled => base,
        RadioState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), HOVER),
        RadioState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), PRESSED),
    }
}

fn segment_label_color(theme: &Theme, selected: bool, state: RadioState) -> Color {
    if matches!(state, RadioState::Disabled) {
        return theme.resolve(ColorRole::OnSurfaceMuted);
    }
    if selected {
        theme.resolve(ColorRole::OnAccent)
    } else {
        theme.resolve(ColorRole::OnSurface)
    }
}

/// Single source of truth for segment labels — drives both the visible
/// `TextNode` and the explicit AccessKit `radio` name so the
/// screen-reader text and the painted text never diverge.
fn segment_label(index: usize) -> &'static str {
    match index {
        0 => "Day",
        1 => "Week",
        2 => "Month",
        _ => "?",
    }
}

struct SegmentedView;

impl WidgetCore for SegmentedView {
    type State = GroupState;
    // Every state change flows through `apply_key` (wire format
    // `"<i>:<EventName>"`) + the InputRouter sub-index dispatch, so no
    // keybinding-channel events flow through `event_name`.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut ext = RadioGroupExternal::new(N);
        // Default selection = segment 0 ("Day"). M3 segmented buttons
        // boot with a selected segment; `KeyboardActivate` selects
        // without leaving a hover / pressed residue (state stays Idle),
        // so the boot frame paints a clean Accent pill on segment 0.
        ext.send(0, RadioEvent::KeyboardActivate);
        Box::new(ext)
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> GroupState {
        let mut out = GroupState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        rc::read_rows(intro, &mut out.rows);
        out.focused = rc::focused_index(intro);
        out
    }

    fn view(state: GroupState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // Keys flow through `apply_key`'s wire format directly; the
        // enum-typed channel stays unused (mirrors hello-radio-group).
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-segmented-button (R728 §5.38 single-select segmented)"
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
        // R51.57 §5.39 — single tab stop through the shared R751 roving-key
        // shell; `resolve_target_index` (arrow / Home / End / shortcut) routes
        // only when the group is focused, so sibling controls keep their
        // keymaps. Selects the target segment (mutual exclusion + `"selected"`
        // intent) on the activate edge.
        rc::roving_key(scene, focused, Self::tag(), key, resolve_target_index)
    }

    fn fmt_state_log(state: &GroupState) -> String {
        let rows = state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (s, sel))| {
                format!(
                    "{i}={}{}",
                    radio_state_short(*s),
                    if *sel { "+" } else { "-" }
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        match state.focused {
            Some(idx) => format!("{rows} focused={idx}"),
            None => rows,
        }
    }
}

impl WidgetA11y for SegmentedView {
    /// §5.40 composite AccessKit tree: one `AriaRole::RadioGroup`
    /// parent + one `AriaRole::RadioButton` per segment (single-select
    /// segmented buttons map to the radiogroup pattern in WAI-ARIA).
    /// Each segment carries an **explicit** name from
    /// [`segment_label`] — explicit names survive `enrich_names_from_scene`
    /// (so the leading check glyph's `TextNode` cannot corrupt the
    /// accessible name) and keep the screen-reader text single-sourced
    /// with the painted label.
    fn access_node(state: &GroupState, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_idx = rc::active_index(&state.rows, state.focused);
        let tags: Vec<String> = (0..N).map(|i| format!("{PRIMARY_TAG}#{i}")).collect();
        let cells: Vec<RadioCell<'_>> = state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (radio_state, selected))| RadioCell {
                tag: &tags[i],
                label: Some(segment_label(i)),
                state: *radio_state,
                selected: *selected,
                focused: group_focused && i == active_idx,
            })
            .collect();
        radiogroup_radio_nodes(<Self as WidgetCore>::tag(), "View mode", &cells)
    }

    /// §5.40 composite focus model — when the group is focused, report
    /// the parent tag as the `TreeUpdate::focus` target and the active
    /// segment as `aria-activedescendant` (APG roving tabindex).
    fn access_focus_target(state: &GroupState, focused: Option<&str>) -> Option<AccessFocus> {
        rc::composite_focus_target(
            <Self as WidgetCore>::tag(),
            focused,
            rc::active_index(&state.rows, state.focused),
        )
    }

    /// §5.40 composite child action dispatch — an AT `Click` / `Default`
    /// on a segment's `NodeId` activates it through the same wire-format
    /// path `apply_key` uses; `Focus` pins the active descendant without
    /// mutating the selection.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        rc::composite_child_invoke(scene, sub_tag, action, N)
    }
}

impl WidgetView for SegmentedView {
    type Renderer = HelloSegmentedButtonRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Resolve a keyboard key to a target segment index given the current
/// selection. `d` / `w` / `m` map directly; `ArrowRight` / `ArrowDown`
/// step forward, `ArrowLeft` / `ArrowUp` step back (wrapping);
/// `Home` / `End` jump to first / last. `None` for unrecognised keys.
fn resolve_target_index(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    match key {
        "d" | "Home" => Some(0),
        "w" => Some(1),
        "m" | "End" => Some(N - 1),
        "ArrowRight" | "ArrowDown" => Some(rc::step(intro, 1, N)),
        "ArrowLeft" | "ArrowUp" => Some(rc::step(intro, -1, N)),
        _ => None,
    }
}

fn radio_state_short(state: RadioState) -> &'static str {
    match state {
        RadioState::Idle => "I",
        RadioState::Hover => "H",
        RadioState::Pressed => "P",
        RadioState::Disabled => "D",
    }
}

fn main() {
    pinion_shell::run::<SegmentedView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;
    use pinion_a11y::{AccessValue, AriaRole};

    fn unselected_state() -> GroupState {
        GroupState::idle()
    }

    fn selected_state(idx: usize) -> GroupState {
        let mut s = unselected_state();
        s.rows[idx].1 = true;
        s
    }

    fn focused_state(focused_idx: usize) -> GroupState {
        let mut s = unselected_state();
        s.focused = Some(focused_idx);
        s
    }

    #[test]
    fn emits_one_group_plus_n_radio_nodes() {
        let nodes = SegmentedView::access_node(&unselected_state(), None);
        assert_eq!(nodes.len(), N + 1);
        assert_eq!(nodes[0].role, AriaRole::RadioGroup);
        assert_eq!(nodes[0].name.as_deref(), Some("View mode"));
        for i in 0..N {
            assert_eq!(nodes[i + 1].role, AriaRole::RadioButton);
        }
    }

    #[test]
    fn group_lists_all_children_in_order() {
        let nodes = SegmentedView::access_node(&unselected_state(), None);
        assert_eq!(nodes[0].children.len(), N);
        for i in 0..N {
            assert_eq!(nodes[0].children[i], format!("view_mode#{i}"));
        }
    }

    #[test]
    fn each_segment_has_explicit_label_name() {
        let nodes = SegmentedView::access_node(&unselected_state(), None);
        assert_eq!(nodes[1].name.as_deref(), Some("Day"));
        assert_eq!(nodes[2].name.as_deref(), Some("Week"));
        assert_eq!(nodes[3].name.as_deref(), Some("Month"));
    }

    #[test]
    fn explicit_names_survive_enrichment_despite_check_glyph() {
        // The selected segment paints a leading check-glyph `TextNode`
        // ahead of the label. Explicit names must not be overwritten by
        // name-from-contents enrichment (else the name becomes the
        // glyph or "<glyph> Day").
        let state = selected_state(0);
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = SegmentedView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[1].name.as_deref(), Some("Day"));
        assert_eq!(nodes[0].name.as_deref(), Some("View mode"));
    }

    #[test]
    fn selected_segment_carries_checked_true() {
        let nodes = SegmentedView::access_node(&selected_state(1), None);
        assert_eq!(nodes[2].state.checked, Some(true));
        assert_eq!(nodes[2].value, Some(AccessValue::Bool(true)));
        assert_eq!(nodes[1].state.checked, Some(false));
        assert_eq!(nodes[3].state.checked, Some(false));
    }

    #[test]
    fn group_focused_marks_active_segment_focused() {
        let nodes = SegmentedView::access_node(&selected_state(1), Some("view_mode"));
        assert!(nodes[2].state.focused);
        assert!(!nodes[1].state.focused);
        assert!(!nodes[3].state.focused);
    }

    #[test]
    fn unselected_group_focuses_first_segment() {
        let nodes = SegmentedView::access_node(&unselected_state(), Some("view_mode"));
        assert!(nodes[1].state.focused);
    }

    #[test]
    fn access_focus_target_composite_parent_focus_with_active_segment() {
        let target = SegmentedView::access_focus_target(&selected_state(2), Some("view_mode"))
            .expect("group focused returns Some");
        assert_eq!(target.focus_tag, "view_mode");
        assert_eq!(target.active_descendant.as_deref(), Some("view_mode#2"));
    }

    #[test]
    fn access_focus_target_atomic_when_sibling_focused() {
        let target = SegmentedView::access_focus_target(&selected_state(1), Some("save_btn"))
            .expect("non-group focused returns Some(atomic)");
        assert_eq!(target.focus_tag, "save_btn");
        assert!(target.active_descendant.is_none());
    }

    #[test]
    fn access_focus_target_none_when_no_focus() {
        assert!(SegmentedView::access_focus_target(&unselected_state(), None).is_none());
    }

    #[test]
    fn active_descendant_honors_focused_over_selected() {
        let mut state = selected_state(0);
        state.focused = Some(2);
        let target = SegmentedView::access_focus_target(&state, Some("view_mode"))
            .expect("group focused returns Some");
        assert_eq!(target.active_descendant.as_deref(), Some("view_mode#2"));
    }

    #[test]
    fn focused_state_marks_correct_segment() {
        let nodes = SegmentedView::access_node(&focused_state(2), Some("view_mode"));
        assert!(nodes[3].state.focused);
        assert!(!nodes[1].state.focused);
        assert!(!nodes[2].state.focused);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;
    use pinion_core::scene::ExternalNode;
    use pinion_core::widgets::radio_group::RadioGroupExternal;

    fn scene() -> Scene {
        let ext = RadioGroupExternal::new(N);
        Scene::External(ExternalNode::new(Box::new(ext)).with_tag(PRIMARY_TAG))
    }

    fn selected_index(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else {
            return None;
        };
        node.handle
            .introspect()?
            .query("selected_index")
            .ok()
            .and_then(|v| match v {
                IntrospectValue::Int(i) => Some(i),
                _ => None,
            })
    }

    #[test]
    fn focused_group_routes_arrow_right_to_selection() {
        let mut s = scene();
        assert!(SegmentedView::apply_key(
            &mut s,
            Some("view_mode"),
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(selected_index(&s), Some(0));
    }

    #[test]
    fn arrow_right_advances_and_wraps() {
        let mut s = scene();
        for expected in [0_i64, 1, 2, 0] {
            assert!(SegmentedView::apply_key(
                &mut s,
                Some("view_mode"),
                "ArrowRight",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(selected_index(&s), Some(expected));
        }
    }

    #[test]
    fn arrow_left_steps_back_and_wraps() {
        let mut s = scene();
        // First left from no-selection lands on the last segment.
        assert!(SegmentedView::apply_key(
            &mut s,
            Some("view_mode"),
            "ArrowLeft",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(selected_index(&s), Some(i64::try_from(N - 1).unwrap()));
    }

    #[test]
    fn no_focus_swallows_arrow_silently() {
        let mut s = scene();
        assert!(!SegmentedView::apply_key(
            &mut s,
            None,
            "ArrowRight",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), None);
    }

    #[test]
    fn other_widget_focused_swallows_arrow_silently() {
        let mut s = scene();
        assert!(!SegmentedView::apply_key(
            &mut s,
            Some("volume_slider"),
            "ArrowRight",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(selected_index(&s), None);
    }

    #[test]
    fn letter_shortcuts_select_named_segment() {
        for (key, idx) in [("d", 0_i64), ("w", 1), ("m", 2)] {
            let mut s = scene();
            assert!(SegmentedView::apply_key(
                &mut s,
                Some("view_mode"),
                key,
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(selected_index(&s), Some(idx), "shortcut {key}");
        }
    }

    #[test]
    fn home_and_end_jump_to_edges() {
        let mut s = scene();
        assert!(SegmentedView::apply_key(
            &mut s,
            Some("view_mode"),
            "End",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(i64::try_from(N - 1).unwrap()));
        assert!(SegmentedView::apply_key(
            &mut s,
            Some("view_mode"),
            "Home",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(0));
    }

    #[test]
    fn access_child_invoke_click_selects_addressed_segment() {
        let mut s = scene();
        assert!(SegmentedView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "1",
            AccessAction::Click
        ));
        assert_eq!(selected_index(&s), Some(1));
    }

    #[test]
    fn access_child_invoke_subsequent_click_switches_selection() {
        let mut s = scene();
        assert!(SegmentedView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "0",
            AccessAction::Click
        ));
        assert_eq!(selected_index(&s), Some(0));
        assert!(SegmentedView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "2",
            AccessAction::Click
        ));
        assert_eq!(selected_index(&s), Some(2));
    }

    #[test]
    fn access_child_invoke_out_of_range_returns_false() {
        let mut s = scene();
        assert!(!SegmentedView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "9",
            AccessAction::Click
        ));
        assert_eq!(selected_index(&s), None);
    }

    #[test]
    fn r55_g18_view_contains_composite_paint_root_tag() {
        // R55.G.18 §5.49 — the paint scene must carry `PRIMARY_TAG` on
        // the track so `{path: "view_mode"}` AI routing + `rect_for_tag`
        // resolve to the segmented control.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<SegmentedView>(
            GroupState::idle(),
            &Frame::default(),
        );
    }
}
