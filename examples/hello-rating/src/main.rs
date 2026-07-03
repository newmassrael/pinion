//! `hello-rating` — R758 §5.38 §5.40 a Material-style **star rating**:
//! a cumulative N-star value selector with hover preview.
//!
//! ## Topology — a radiogroup of "N stars", reused
//!
//! A whole-star rating is a **discrete value selector**: picking the 3rd
//! star means "3 stars", exactly one value at a time. That is the
//! WAI-ARIA *radiogroup* shape (each star is the radio "rate N stars" —
//! the model MUI's `Rating` and most design systems lower to), so this
//! binding is pure **composition** over the R51.44
//! [`RadioGroupExternal`] coordinator — the same `hello-segmented-button`
//! (R728) / `hello-tabs` reuse — and adds **zero new interaction
//! substrate**: mutual exclusion, the roving-tabindex keyboard, the
//! `"selected"` intent, and the AccessKit `radiogroup` + `radio` tree all
//! come from the framework. A *continuous / half-star* rating is the
//! other textbook school (a `role="slider"` with `aria-valuenow` — the
//! `SliderExternal` axis); the two are **peers**, and the slider variant
//! is a deferred carry until a consumer needs fractional values
//! ([[abstraction-needs-second-consumer]] / the R745 two-schools rule).
//!
//! The keyboard correctly reuses the **single-tab-stop**
//! [`rc::roving_key`] shell (the whole rating is one Tab stop, arrows
//! move an internal cursor) — distinct from the **per-item-tab-stop**
//! `focus_request::rove` axis the R757 cards use (each card its own Tab
//! stop). Same word ("roving"), two orthogonal mechanisms; this binding
//! sits on the radio side.
//!
//! ## What is genuinely new: the cumulative star paint (1st consumer)
//!
//! The distinctive rating affordance is **cumulative fill with hover
//! preview**: stars `0..=k` paint filled while `k` is the hovered star
//! (a live preview of the rating a click would commit), falling back to
//! the committed selection when nothing is hovered. That fill logic +
//! the star glyphs (`★` / `☆`) are the only new code, built **inline**
//! here as the 1st star-paint consumer (the R753/R756 inline-first rule);
//! a 2nd star consumer would lift it to `pinion_widget_paint`. The hover
//! *feedback* is the fill-extent change itself, so the stars need no
//! per-cell state-layer box — the rating's distinctive expression
//! replaces the generic overlay (the R739 "make the duplication
//! unnecessary" rule).
//!
//! Each star is tagged `"rating#<i>"` so the `InputRouter` R51.42
//! sub-index split routes a cursor on star `i` to
//! `invoke("send", Text("<i>:<EventName>"))` against the single composite
//! handle. AI clients reach the same surface through
//! `query("selected_index")`, the per-star `query("state.<i>")` /
//! `query("selected.<i>")` paths, the AccessKit tree, and the identical
//! `invoke "send"` wire the keyboard path uses.
//!
//! Keyboard (`apply_key`, single Tab stop): `ArrowRight` / `ArrowUp`
//! raise the rating one star, `ArrowLeft` / `ArrowDown` lower it (with
//! wrap), `Home` / `End` jump to 1 / N stars, and the digit keys
//! `1`..`5` set that many stars directly.

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, RadioCell, WidgetA11y, radiogroup_radio_nodes,
};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::radio::{RadioEvent, RadioState};
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::radio_composite as rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRatingRenderer, HelloRatingRendererError);

const WIN_W: u32 = 320;
const WIN_H: u32 = 140;
/// [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key — the `"app"` convention shared across the
/// example gallery.
const THEME_TAG: &str = "app";
/// Star count — a 5-star rating, the near-universal scale.
const N: usize = 5;
const PRIMARY_TAG: &str = "rating";

/// Per-star square hit-box side, and the glyph point size inside it.
const STAR_BOX: u32 = 48;
const STAR_FONT_PX: u32 = 34;
/// Boot rating — 3 of 5 stars, so the first frame shows a meaningful
/// (non-empty) value the way an M3 segmented button boots selected.
const BOOT_INDEX: usize = 2;

/// U+2605 BLACK STAR — a filled star. Named const + escape per the
/// non-ASCII-source rule (raw glyph only in doc strings).
const STAR_FILLED: &str = "\u{2605}";
/// U+2606 WHITE STAR — an empty (outline) star.
const STAR_EMPTY: &str = "\u{2606}";

/// Cached projection of the group: one `(RadioState, selected)` pair per
/// star plus the §5.40 AT-side active-descendant index. `Copy` because
/// both fields' inner types are `Copy`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct RatingState {
    rows: [(RadioState, bool); N],
    focused: Option<usize>,
}

impl RatingState {
    fn idle() -> Self {
        Self {
            rows: [(RadioState::Idle, false); N],
            focused: None,
        }
    }
}

/// The index up to which stars paint filled: the **hovered** (or pressed)
/// star previews the rating a click would commit; with no pointer over
/// the row the **committed** selection drives the fill. `None` ⇒ no star
/// is filled (an unrated, un-hovered row).
fn fill_extent(rows: &[(RadioState, bool); N]) -> Option<usize> {
    if let Some(hovered) = rows
        .iter()
        .position(|(s, _)| matches!(s, RadioState::Hover | RadioState::Pressed))
    {
        return Some(hovered);
    }
    rows.iter().position(|(_, selected)| *selected)
}

/// view-fn (§6.3): pure sync `RatingState -> Scene`. A centred row of N
/// stars, filled cumulatively up to [`fill_extent`].
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RatingState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let extent = fill_extent(&state.rows);
    let stars: Vec<Scene> = (0..N)
        .map(|i| star(i, extent.is_some_and(|e| i <= e), &theme))
        .collect();
    let row = Scene::Container(
        ContainerNode::new(stars).with_tag(PRIMARY_TAG).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                // (R1030 §5.39) A rating is a single radiogroup Tab stop. This
                // is a hand-composed widget (a raw Container over
                // `RadioGroupExternal`, not a focus-aware helper), so the
                // composing view owns the focus opt-in — there is no widget
                // style default to carry it.
                .with_focusable(true)
                .with_gap(4),
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

/// One star cell — a fixed square hit-box (tagged `"rating#<i>"` for
/// R51.42 sub-index dispatch) holding the filled or empty glyph. The
/// glyph is `Presentational`: the AT name is the explicit
/// [`star_label`] on the `radio` node, never the decorative star.
fn star(index: usize, filled: bool, theme: &Theme) -> Scene {
    let (glyph, color) = if filled {
        (STAR_FILLED, theme.resolve(ColorRole::Accent))
    } else {
        (STAR_EMPTY, theme.resolve(ColorRole::OnSurfaceMuted))
    };
    let star_tag = format!("{PRIMARY_TAG}#{index}");
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(
            TextNode::styled(
                glyph,
                Rect::default(),
                TextStyle::new().with_size_px(STAR_FONT_PX).with_fg(color),
            )
            .with_role(TextRole::Presentational),
        )])
        .with_tag(star_tag)
        .with_style(BoxStyle::filled(Color::rgba(0, 0, 0, 0)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(STAR_BOX, STAR_BOX)),
        ),
    )
}

/// Single source of truth for each star's AT accessible name — drives the
/// explicit `radio` name so the screen-reader text ("3 Stars") is
/// single-sourced and never the decorative glyph.
fn star_label(index: usize) -> &'static str {
    match index {
        0 => "1 Star",
        1 => "2 Stars",
        2 => "3 Stars",
        3 => "4 Stars",
        4 => "5 Stars",
        _ => "?",
    }
}

struct RatingView;

impl WidgetCore for RatingView {
    type State = RatingState;
    // Every state change flows through `apply_key` (wire format
    // `"<i>:<EventName>"`) + the InputRouter sub-index dispatch, so no
    // keybinding-channel events flow through `event_name` (mirror of
    // hello-segmented-button / hello-radio-group).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut ext = RadioGroupExternal::new(N);
        // Boot at BOOT_INDEX via the keyboard activate edge so the first
        // frame paints a committed rating with no hover / pressed
        // residue (state stays Idle), exactly as the segmented button
        // seeds its default segment.
        ext.send(BOOT_INDEX, RadioEvent::KeyboardActivate);
        Box::new(ext)
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> RatingState {
        let mut out = RatingState::idle();
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

    fn view(state: RatingState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-rating (R758 §5.38 cumulative star rating)"
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
        // Single tab stop through the shared R751 roving-key shell;
        // `resolve_target_index` (arrow / Home / End / digit) routes only
        // when the rating is focused, so sibling controls keep their
        // keymaps. Selects the target star (mutual exclusion + `"selected"`
        // intent) on the activate edge.
        rc::roving_key(scene, focused, Self::tag(), key, resolve_target_index)
    }

    fn fmt_state_log(state: &RatingState) -> String {
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

impl WidgetA11y for RatingView {
    /// §5.40 composite AccessKit tree: one `AriaRole::RadioGroup` parent
    /// ("Rating") + one `AriaRole::RadioButton` per star, named "N
    /// Stars" so the screen-reader announces the value choice rather than
    /// the decorative glyph. Explicit names survive
    /// `enrich_names_from_scene`, so the `Presentational` star glyph can
    /// never corrupt the accessible name.
    fn access_node(state: &RatingState, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_idx = rc::active_index(&state.rows, state.focused);
        let tags: Vec<String> = (0..N).map(|i| format!("{PRIMARY_TAG}#{i}")).collect();
        let cells: Vec<RadioCell<'_>> = state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (radio_state, selected))| RadioCell {
                tag: &tags[i],
                label: Some(star_label(i)),
                state: *radio_state,
                selected: *selected,
                focused: group_focused && i == active_idx,
            })
            .collect();
        radiogroup_radio_nodes(<Self as WidgetCore>::tag(), "Rating", &cells)
    }

    /// §5.40 composite focus model — when the rating is focused, report
    /// the parent tag as the `TreeUpdate::focus` target and the active
    /// star as `aria-activedescendant` (APG roving tabindex). The
    /// focus-model policy is the shared [`rc::composite_focus_target`]
    /// SSOT (R758 lift); this caller only supplies the active index.
    fn access_focus_target(state: &RatingState, focused: Option<&str>) -> Option<AccessFocus> {
        rc::composite_focus_target(
            <Self as WidgetCore>::tag(),
            focused,
            rc::active_index(&state.rows, state.focused),
        )
    }

    /// §5.40 composite child action dispatch — an AT `Click` / `Default`
    /// on a star's `NodeId` selects it through the same wire-format path
    /// `apply_key` uses; `Focus` pins the active descendant. The
    /// external-descent + dispatch is the shared
    /// [`rc::composite_child_invoke`] SSOT (R758 lift).
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        rc::composite_child_invoke(scene, sub_tag, action, N)
    }
}

impl WidgetView for RatingView {
    type Renderer = HelloRatingRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Resolve a keyboard key to a target star index given the current
/// rating. Digit keys `1`..`5` set that many stars directly;
/// `ArrowRight` / `ArrowUp` raise the rating, `ArrowLeft` / `ArrowDown`
/// lower it (wrapping); `Home` / `End` jump to 1 / N stars. `None` for
/// unrecognised keys (so sibling keymaps still see them).
fn resolve_target_index(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    match key {
        "1" | "Home" => Some(0),
        "2" => Some(1),
        "3" => Some(2),
        "4" => Some(3),
        "5" | "End" => Some(N - 1),
        "ArrowRight" | "ArrowUp" => Some(rc::step(intro, 1, N)),
        "ArrowLeft" | "ArrowDown" => Some(rc::step(intro, -1, N)),
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
    pinion_shell::run::<RatingView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::external::IntrospectValue;
    use pinion_core::scene::ExternalNode;

    // ── fill_extent (cumulative + hover preview) ──────────────────────

    fn rows_with(selected: Option<usize>, hovered: Option<usize>) -> [(RadioState, bool); N] {
        let mut rows = [(RadioState::Idle, false); N];
        if let Some(s) = selected {
            rows[s].1 = true;
        }
        if let Some(h) = hovered {
            rows[h].0 = RadioState::Hover;
        }
        rows
    }

    #[test]
    fn fill_follows_committed_selection_when_no_hover() {
        assert_eq!(fill_extent(&rows_with(Some(2), None)), Some(2));
        assert_eq!(
            fill_extent(&rows_with(None, None)),
            None,
            "unrated + un-hovered = empty"
        );
    }

    #[test]
    fn hover_preview_overrides_committed_selection() {
        // Committed 3 stars (idx 2), hovering the 5th star (idx 4):
        // the preview wins, filling up to 4.
        assert_eq!(fill_extent(&rows_with(Some(2), Some(4))), Some(4));
        // Hovering a lower star previews fewer filled stars.
        assert_eq!(fill_extent(&rows_with(Some(2), Some(0))), Some(0));
    }

    #[test]
    fn view_fills_cumulatively_up_to_extent() {
        let mut state = RatingState::idle();
        state.rows[2].1 = true; // 3 stars committed
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let (filled, empty) = count_star_glyphs(&scene);
        assert_eq!(filled, 3, "3 stars filled cumulatively");
        assert_eq!(empty, 2, "2 stars empty");
    }

    #[test]
    fn view_carries_primary_and_every_star_tag() {
        let scene = pinion_core::Owner::new().run(|| view(RatingState::idle(), &Frame::new()));
        assert!(scene.contains_tag(PRIMARY_TAG));
        for i in 0..N {
            assert!(
                scene.contains_tag(&format!("{PRIMARY_TAG}#{i}")),
                "star {i} tagged"
            );
        }
    }

    #[test]
    fn r55_g18_view_contains_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<RatingView>(
            RatingState::idle(),
            &Frame::default(),
        );
    }

    // ── keyboard (single-tab-stop roving via rc) ──────────────────────

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
            .and_then(|v| match v {
                IntrospectValue::Int(i) => Some(i),
                _ => None,
            })
    }

    #[test]
    fn digit_keys_set_the_star_count_directly() {
        for (key, idx) in [("1", 0_i64), ("2", 1), ("3", 2), ("4", 3), ("5", 4)] {
            let mut s = scene();
            assert!(RatingView::apply_key(
                &mut s,
                Some(PRIMARY_TAG),
                key,
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(selected_index(&s), Some(idx), "digit {key} sets {idx}");
        }
    }

    #[test]
    fn arrow_up_raises_and_arrow_down_lowers_the_rating() {
        let mut s = scene();
        assert!(RatingView::apply_key(
            &mut s,
            Some(PRIMARY_TAG),
            "3",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(2));
        assert!(RatingView::apply_key(
            &mut s,
            Some(PRIMARY_TAG),
            "ArrowUp",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(3), "ArrowUp raises to 4 stars");
        assert!(RatingView::apply_key(
            &mut s,
            Some(PRIMARY_TAG),
            "ArrowDown",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(selected_index(&s), Some(2), "ArrowDown lowers to 3 stars");
    }

    #[test]
    fn home_and_end_jump_to_one_and_five_stars() {
        let mut s = scene();
        assert!(RatingView::apply_key(
            &mut s,
            Some(PRIMARY_TAG),
            "End",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(i64::try_from(N - 1).unwrap()));
        assert!(RatingView::apply_key(
            &mut s,
            Some(PRIMARY_TAG),
            "Home",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(0));
    }

    #[test]
    fn keys_ignored_when_rating_not_focused() {
        let mut s = scene();
        assert!(!RatingView::apply_key(
            &mut s,
            None,
            "3",
            pinion_core::Modifiers::empty()
        ));
        assert!(!RatingView::apply_key(
            &mut s,
            Some("other_widget"),
            "ArrowUp",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(selected_index(&s), None, "no selection mutated");
    }

    // ── a11y ──────────────────────────────────────────────────────────

    fn selected_state(idx: usize) -> RatingState {
        let mut s = RatingState::idle();
        s.rows[idx].1 = true;
        s
    }

    #[test]
    fn emits_one_radiogroup_plus_n_radio_nodes_named_n_stars() {
        let nodes = RatingView::access_node(&RatingState::idle(), None);
        assert_eq!(nodes.len(), N + 1);
        assert_eq!(nodes[0].role, AriaRole::RadioGroup);
        assert_eq!(nodes[0].name.as_deref(), Some("Rating"));
        for i in 0..N {
            assert_eq!(nodes[i + 1].role, AriaRole::RadioButton);
            assert_eq!(nodes[i + 1].name.as_deref(), Some(star_label(i)));
        }
    }

    #[test]
    fn selected_star_carries_checked_true_only_on_that_radio() {
        let nodes = RatingView::access_node(&selected_state(2), None);
        assert_eq!(nodes[3].state.checked, Some(true), "3rd star radio checked");
        assert_eq!(nodes[1].state.checked, Some(false));
        assert_eq!(nodes[5].state.checked, Some(false));
    }

    #[test]
    fn explicit_star_names_survive_enrichment_despite_glyphs() {
        let state = selected_state(2);
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = RatingView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[0].name.as_deref(), Some("Rating"));
        assert_eq!(nodes[3].name.as_deref(), Some("3 Stars"));
    }

    #[test]
    fn group_focused_marks_active_star_focused() {
        let nodes = RatingView::access_node(&selected_state(1), Some(PRIMARY_TAG));
        assert!(nodes[2].state.focused, "active (selected) star focused");
        assert!(!nodes[1].state.focused);
        assert!(!nodes[3].state.focused);
    }

    #[test]
    fn access_child_invoke_click_selects_addressed_star() {
        let mut s = scene();
        assert!(RatingView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "3",
            AccessAction::Click
        ));
        assert_eq!(
            selected_index(&s),
            Some(3),
            "AT click on 4th star = 4 stars"
        );
    }

    // ── helpers ───────────────────────────────────────────────────────

    fn count_star_glyphs(scene: &Scene) -> (usize, usize) {
        fn walk(scene: &Scene, filled: &mut usize, empty: &mut usize) {
            match scene {
                Scene::Text(t) => {
                    if t.content == STAR_FILLED {
                        *filled += 1;
                    } else if t.content == STAR_EMPTY {
                        *empty += 1;
                    }
                }
                Scene::Container(c) => {
                    for child in &c.children {
                        walk(child, filled, empty);
                    }
                }
                _ => {}
            }
        }
        let (mut filled, mut empty) = (0, 0);
        walk(scene, &mut filled, &mut empty);
        (filled, empty)
    }
}
