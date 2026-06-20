//! `hello-accordion-single` — R701 §5.38 **single-open** WAI-ARIA APG
//! accordion: the GUI consumer of the R700 [`DisclosureGroup`]
//! single-expand coordinator.
//!
//! An accordion is a vertically stacked set of [`Disclosure`] sections,
//! each a header `button` carrying `aria-expanded` that reveals its own
//! content panel. The WAI-ARIA APG "Accordion" pattern defines two
//! variants: **multi-open** (more than one panel open at once — the
//! default, shipped as `examples/hello-accordion` over N independent
//! [`DisclosureExternal`]s) and **single-open** (at most one panel
//! open; opening one collapses whichever was open). That single-open
//! exclusion is *cross-section coordination*, so — like a
//! `RadioGroup`'s sibling-deselect (R51.15) — it is framework-owned,
//! not an application loop. R700 landed the
//! [`DisclosureGroup`](pinion_core::widgets::disclosure_group::DisclosureGroup)
//! coordinator (substrate-first); this binary is its visible consumer.
//!
//! ## Composition — one composite External, not N
//!
//! Unlike multi-open `hello-accordion` (one [`DisclosureExternal`] per
//! section through `create_extra_externals`), the single-open accordion
//! holds **one** [`DisclosureGroupExternal`] at the
//! [`PRIMARY_TAG`] composite paint root — mirroring `hello-radio-group`
//! over [`RadioGroupExternal`]. Each section header row is tagged
//! `"accordion_single#<i>"` (the R51.41 composite paint convention), so
//! the [`InputRouter`](pinion_runtime::input::InputRouter) R51.42
//! `'#'`-split routes a cursor on header `i` to
//! `invoke("send", Text("<i>:<EventName>"))` against the single
//! coordinator. The coordinator enforces "at most one expanded" on the
//! activate edge and emits the §5.20 `"expanded"` intent (new index, or
//! `Null` on collapse-to-none) exactly as `RadioGroup` emits
//! `"selected"`.
//!
//! ## Keyboard model (WAI-ARIA APG accordion)
//!
//! Each header is its **own Tab stop** — all N composite sub-tags
//! (`"accordion_single#0..2"`) are marked `.with_focusable(true)` (the
//! scene-derived §5.39 Tab stops), unlike a
//! `RadioGroup`, which is a single tab stop with internal active-
//! descendant roving. So:
//! - `Tab` / `Shift+Tab` move between section headers (the shell's
//!   focus traversal over the `.with_focusable(true)` tags);
//! - `Space` / `Enter` toggle the focused section (the disclosure
//!   keyboard model — both keys), routed through the coordinator's
//!   `"<i>:KeyboardActivate"` wire format so single-open exclusion
//!   applies identically to a click;
//! - `ArrowDown` / `ArrowUp` move focus to the next / previous header
//!   (wrapping), `Home` / `End` jump to the first / last — the optional
//!   APG arrow navigation, routed through the R664
//!   [`pinion_core::focus_request`] mailbox so the move funnels through
//!   the same `FocusManager::focus_set` path a mouse press uses. Arrow
//!   roving moves *focus only*; it never toggles expansion.
//!
//! Because the headers are composite sub-tags, click-to-focus and the
//! RPC `focus/set` both land focus directly on `"accordion_single#<i>"`
//! (a real [`AccessNode`]), so the default atomic
//! [`access_focus_target`](WidgetA11y::access_focus_target) is correct
//! — no active-descendant redirect is needed (the divergence from
//! `hello-radio-group`, whose tab stop is the parent group).
//!
//! Visual focus indication on the header rows reuses the shared
//! `view_disclosure` paint and inherits the R694 focus-ring carry
//! (composite paint widgets do not yet draw a ring); the focus model is
//! nonetheless fully AT-reported (`access_node` marks the focused
//! header) and RPC-verifiable (`focus/get`).

use pinion_a11y::{AccessAction, AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::disclosure::DisclosureState;
use pinion_core::widgets::disclosure_group::DisclosureGroupExternal;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::disclosure::{view_disclosure, DisclosureStyle};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloAccordionSingleRenderer, HelloAccordionSingleRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 440;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across
/// the example gallery.
const THEME_TAG: &str = "app";

/// Section count. Three is enough to exercise wrap-around arrow roving
/// (last -> first) and to read as a real settings accordion.
const N: usize = 3;

/// Composite paint root tag carrying the single
/// [`DisclosureGroupExternal`]. The per-header rows are tagged
/// `"accordion_single#<i>"`; the R51.42 `'#'`-split routes a click on
/// row `i` to the coordinator's `"<i>:<EventName>"` send.
const PRIMARY_TAG: &str = "accordion_single";

/// Per-section header dispatch tags — the composite sub-tag form
/// `"accordion_single#<i>"`. `&'static str` (not `format!`) because each
/// header is marked `.with_focusable(true)` in the view fn (the
/// scene-derived §5.39 Tab stops, so all N are stops). Each lands on one
/// section header via [`view_disclosure`], and the input router
/// hit-tests a click on header `i` straight to this composite tag.
const ROW_TAGS: [&str; N] =
    ["accordion_single#0", "accordion_single#1", "accordion_single#2"];

/// Header summary labels — double as each section's AT accessible name
/// (the disclosure twisty is presentational, so name-from-contents
/// enrichment lands here).
const SUMMARIES: [&str; N] = ["General", "Privacy", "Advanced"];

/// Panel body copy, shown only while the section is expanded.
const BODIES: [&str; N] = [
    "Theme, language, and startup behaviour.",
    "Telemetry, crash reports, and data retention.",
    "Developer flags and experimental features.",
];

/// Paint tags on the per-section panel bodies so the AI-first demo can
/// confirm — via `scene/bbox` — that section `i`'s body is present in
/// the scene only while that section is expanded.
const BODY_TAGS: [&str; N] =
    ["accordion_single_body_0", "accordion_single_body_1", "accordion_single_body_2"];

/// Cached projection: one `(DisclosureState, expanded)` pair per
/// section, read from the single [`DisclosureGroupExternal`]'s per-
/// section introspect slots. `Copy` because both inner types are
/// `Copy`, so the shell hands the snapshot into the paint closure
/// without lifetime gymnastics. Single-open is a coordinator invariant,
/// not a projection one: at most one pair carries `expanded == true`.
type AccordionState = [(DisclosureState, bool); N];

/// Parse the focused composite sub-tag (`"accordion_single#<i>"`) into a
/// section index. Returns `None` for a non-matching focus tag so
/// `apply_key` declines keys when one of our headers does not own focus.
fn focused_section(focused: Option<&str>) -> Option<usize> {
    ROW_TAGS.iter().position(|t| Some(*t) == focused)
}

/// view-fn (§6.3): pure sync mapping [`AccordionState`] -> [`Scene`].
/// A vertical column of N [`view_disclosure`] sections, each tagged
/// `"accordion_single#<i>"` on its header (the composite click target)
/// with a tagged panel body shown only while expanded. The column
/// carries [`PRIMARY_TAG`] so the composite root is paint-addressable
/// (`{path: "accordion_single"}` AI input routing + `rect_for_tag` AT
/// bounds) — the same wrapper convention `hello-radio-group` uses.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &AccordionState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    // (R1020 §5.39) Each header is its own Tab stop — `.with_focusable(true)`
    // marks all of `ROW_TAGS`, so the shared header style opts into
    // the scene-derived focus enumeration.
    let style = DisclosureStyle::m3().with_focusable(true);
    let sections: Vec<Scene> = (0..N)
        .map(|i| {
            let (interaction, expanded) = state[i];
            let body = Scene::Text(
                TextNode::styled(
                    BODIES[i],
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(14)
                        .with_fg(theme.resolve(ColorRole::OnSurface)),
                )
                .with_tag(BODY_TAGS[i]),
            );
            view_disclosure(
                ROW_TAGS[i],
                interaction,
                expanded,
                &theme,
                &style,
                SUMMARIES[i],
                body,
            )
        })
        .collect();
    let column = Scene::Container(
        ContainerNode::new(sections)
            .with_tag(PRIMARY_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    // Gap between stacked sections so the accordion reads
                    // as discrete cards (M3 list-section spacing).
                    .with_gap(8),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![column])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

pub struct AccordionSingleView;

impl WidgetCore for AccordionSingleView {
    type State = AccordionState;
    // Every state change flows through `apply_key` (keyboard) or the
    // InputRouter's composite `"<i>:<EventName>"` dispatch (pointer),
    // never the shell's enum-typed `keybinding` channel — so `()`
    // satisfies the trait's `Copy` bound without an unused event variant
    // (mirror of `hello-radio-group` / `hello-accordion`).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(DisclosureGroupExternal::new(N))
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> AccordionState {
        let mut out: AccordionState = [(DisclosureState::Idle, false); N];
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        // Each section's interaction state + expanded bit comes from the
        // R700 per-section query paths (`state.<i>` / `expanded.<i>`).
        // The introspect channel is the single source of truth: an AI
        // client running `scene/query /accordion_single/external/state.0`
        // sees exactly the value the view fn renders.
        for (i, slot) in out.iter_mut().enumerate() {
            let state = match intro.query(&format!("state.{i}")) {
                Some(IntrospectValue::Text(name)) => DisclosureState::from_name_or_default(&name),
                _ => DisclosureState::Idle,
            };
            let expanded = matches!(
                intro.query(&format!("expanded.{i}")),
                Some(IntrospectValue::Bool(true)),
            );
            *slot = (state, expanded);
        }
        out
    }

    fn view(state: AccordionState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // No typed events reach the keybinding channel; the single
        // source of truth for the keymap lives in `apply_key`.
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-accordion-single (R701 §5.38 single-open APG accordion)"
    }


    /// WAI-ARIA APG accordion keyboard model, gated on the focused
    /// header (roving-tabindex `apply_key` discipline — keys route only
    /// when one of our headers owns focus):
    ///
    /// - `Space` / `Enter` — toggle the focused section (both keys, the
    ///   disclosure model), through the coordinator's
    ///   `"<i>:KeyboardActivate"` wire format so single-open exclusion
    ///   collapses whichever sibling was open;
    /// - `ArrowDown` / `ArrowUp` — move focus to the next / previous
    ///   header, wrapping at the ends;
    /// - `Home` / `End` — move focus to the first / last header.
    ///
    /// Focus moves are requested through the R664
    /// [`pinion_core::focus_request`] mailbox; the shell drains it after
    /// this dispatch and applies it via the same `FocusManager` path a
    /// mouse press uses, so `External::on_focus_change` observers fire
    /// identically. Arrow roving moves focus only — never expansion.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let Some((idx, focused_tag)) = focused_section(focused).zip(focused) else {
            return false;
        };
        if matches!(key, "Space" | "Enter") {
            // Composite activation: one External, the focused row's
            // `{idx}:KeyboardActivate` sub-event. This stays per-binding
            // (the composite sub-index wire form is local).
            let Scene::External(node) = scene else {
                return false;
            };
            let Some(intro) = node.handle.introspect_mut() else {
                return false;
            };
            return intro
                .invoke("send", IntrospectValue::Text(format!("{idx}:KeyboardActivate")))
                .is_ok();
        }
        // R757 §5.39 — the WAI-ARIA roving-tabindex navigation is the
        // shared [`pinion_core::focus_request::rove`] SSOT.
        pinion_core::focus_request::rove(ROW_TAGS.as_slice(), focused_tag, key)
    }

    fn fmt_state_log(state: &AccordionState) -> String {
        state
            .iter()
            .enumerate()
            .map(|(i, (s, expanded))| {
                format!("{i}={}{}", s.as_name(), if *expanded { "+" } else { "-" })
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl WidgetA11y for AccordionSingleView {
    /// R701 §5.40 — flat contribution of N `button` nodes, one per
    /// section header, mirroring multi-open `hello-accordion`. Each
    /// carries `aria-expanded` (the R696 `AccessNode::with_expanded`
    /// axis), the interaction state flags, and `focused` when that
    /// header owns shell focus. No parent container node — WAI-ARIA
    /// defines no "accordion" role; the section headers are plain
    /// button controls in document order (each a Tab stop), so the
    /// accessible names come from name-from-contents enrichment of each
    /// header's summary label.
    ///
    /// The single-open invariant lives in the coordinator, not the a11y
    /// tree: at most one node reports `aria-expanded="true"`, but each
    /// node is an independent `button` (no `radiogroup`/`radio`
    /// semantics — accordion headers are not radios).
    fn access_node(state: &AccordionState, focused: Option<&str>) -> Vec<AccessNode> {
        (0..N)
            .map(|i| {
                let (interaction, expanded) = state[i];
                AccessNode::new(ROW_TAGS[i], AriaRole::Button)
                    .with_expanded(expanded)
                    .with_state(AccessState {
                        focused: focused == Some(ROW_TAGS[i]),
                        ..AccessState::from_interaction(interaction, None)
                    })
            })
            .collect()
    }

    /// R701 §5.40 — composite child action dispatch. The header nodes
    /// carry composite tags (`"accordion_single#<i>"`), so an AT
    /// `Click` / `Default` on a header splits at `'#'` and arrives here
    /// with the sub-tag index. Activation runs the pointer cycle
    /// (`PointerEnter` / `Down` / `Up` / `Leave`) against the
    /// coordinator's `"<i>:<EventName>"` wire format — the same path a
    /// real click takes, so single-open exclusion holds. `Focus` is
    /// accepted (`true`) without mutation; the accordion has no active-
    /// descendant model (each header is its own Tab stop), so there is
    /// nothing further to update. Other actions decline so the shell
    /// keeps its fallback chain.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Ok(idx) = sub_tag.parse::<usize>() else {
            return false;
        };
        if idx >= N {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match action {
            AccessAction::Click | AccessAction::Default => {
                for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                    let _ = intro.invoke("send", IntrospectValue::Text(format!("{idx}:{ev}")));
                }
                true
            }
            AccessAction::Focus => true,
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for AccordionSingleView {
    type Renderer = HelloAccordionSingleRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<AccordionSingleView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn idle() -> AccordionState {
        [(DisclosureState::Idle, false); N]
    }

    fn with_expanded(idx: usize) -> AccordionState {
        let mut s = idle();
        s[idx].1 = true;
        s
    }

    /// Build a fresh single-open accordion scene mirroring `create_external`
    /// so `apply_key` / `access_child_invoke` walk the live shell's exact
    /// topology (one composite `Scene::External` at `PRIMARY_TAG`).
    fn scene_fixture() -> Scene {
        Scene::External(
            ExternalNode::new(AccordionSingleView::create_external()).with_tag(PRIMARY_TAG),
        )
    }

    fn expanded_index(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else {
            return None;
        };
        match node.handle.introspect()?.query("expanded_index") {
            Some(IntrospectValue::Int(i)) => Some(i),
            _ => None,
        }
    }

    // ── view / composition ────────────────────────────────────────

    #[test]
    fn view_carries_primary_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<AccordionSingleView>(
            idle(),
            &Frame::new(),
        );
    }

    #[test]
    fn view_carries_every_section_header_tag() {
        let scene = pinion_core::Owner::new().run(|| view(&idle(), &Frame::new()));
        for tag in ROW_TAGS {
            assert!(scene.contains_tag(tag), "header tag {tag} present");
        }
    }

    #[test]
    fn collapsed_section_omits_its_body_expanded_section_includes_it() {
        // Only section 1 expanded: body 1 present, bodies 0 and 2 absent.
        let scene = pinion_core::Owner::new().run(|| view(&with_expanded(1), &Frame::new()));
        assert!(!scene.contains_tag(BODY_TAGS[0]), "collapsed section 0 hides body");
        assert!(scene.contains_tag(BODY_TAGS[1]), "expanded section 1 shows body");
        assert!(!scene.contains_tag(BODY_TAGS[2]), "collapsed section 2 hides body");
    }

    // ── focus model ───────────────────────────────────────────────

    #[test]
    fn every_header_is_a_tab_stop() {
        // §5.39: tree order IS tab order — collected from the paint scene.
        let scene = pinion_core::Owner::new().run(|| view(&idle(), &Frame::new()));
        assert_eq!(
            scene.collect_focusable_tags(),
            ROW_TAGS.iter().map(|t| (*t).to_owned()).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn arrow_down_requests_next_header_wrapping() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain(); // clear any prior
        assert!(AccordionSingleView::apply_key(
            &mut scene,
            Some(ROW_TAGS[N - 1]),
            "ArrowDown",
            pinion_core::Modifiers::empty(),
        ));
        // Wraps from the last header back to the first.
        assert_eq!(
            pinion_core::focus_request::drain().as_deref(),
            Some(ROW_TAGS[0]),
        );
    }

    #[test]
    fn arrow_up_requests_previous_header_wrapping() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(AccordionSingleView::apply_key(
            &mut scene,
            Some(ROW_TAGS[0]),
            "ArrowUp",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(
            pinion_core::focus_request::drain().as_deref(),
            Some(ROW_TAGS[N - 1]),
        );
    }

    #[test]
    fn home_and_end_jump_to_first_and_last() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(AccordionSingleView::apply_key(
            &mut scene,
            Some(ROW_TAGS[1]),
            "Home",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(ROW_TAGS[0]));
        assert!(AccordionSingleView::apply_key(
            &mut scene,
            Some(ROW_TAGS[1]),
            "End",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(ROW_TAGS[N - 1]));
    }

    #[test]
    fn arrow_keys_ignored_when_no_header_focused() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(!AccordionSingleView::apply_key(
            &mut scene,
            None,
            "ArrowDown",
            pinion_core::Modifiers::empty(),
        ));
        assert!(!AccordionSingleView::apply_key(
            &mut scene,
            Some("some_sibling"),
            "ArrowDown",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain(), None, "no focus request emitted");
    }

    // ── single-open exclusion via keyboard ────────────────────────

    #[test]
    fn space_expands_focused_section() {
        let mut scene = scene_fixture();
        assert!(AccordionSingleView::apply_key(
            &mut scene,
            Some(ROW_TAGS[1]),
            "Space",
            pinion_core::Modifiers::empty(),
        ));
        let s = AccordionSingleView::read_state(&scene);
        assert!(s[1].1, "focused section 1 expands");
        assert!(!s[0].1 && !s[2].1, "others stay collapsed");
        assert_eq!(expanded_index(&scene), Some(1));
    }

    #[test]
    fn enter_also_toggles_focused_section() {
        let mut scene = scene_fixture();
        assert!(AccordionSingleView::apply_key(
            &mut scene,
            Some(ROW_TAGS[0]),
            "Enter",
            pinion_core::Modifiers::empty(),
        ));
        assert!(AccordionSingleView::read_state(&scene)[0].1, "Enter expands section 0");
    }

    #[test]
    fn keyboard_open_collapses_previously_open_section() {
        let mut scene = scene_fixture();
        // Open section 0 via Space.
        let _ = AccordionSingleView::apply_key(
            &mut scene,
            Some(ROW_TAGS[0]),
            "Space",
            pinion_core::Modifiers::empty(),
        );
        assert_eq!(expanded_index(&scene), Some(0));
        // Open section 2 — single-open exclusion collapses section 0.
        let _ = AccordionSingleView::apply_key(
            &mut scene,
            Some(ROW_TAGS[2]),
            "Space",
            pinion_core::Modifiers::empty(),
        );
        let s = AccordionSingleView::read_state(&scene);
        assert!(s[2].1, "section 2 now open");
        assert!(!s[0].1, "section 0 collapsed by single-open exclusion");
        assert_eq!(expanded_index(&scene), Some(2));
    }

    // ── a11y ──────────────────────────────────────────────────────

    #[test]
    fn emits_n_button_nodes_with_aria_expanded() {
        let nodes = AccordionSingleView::access_node(&idle(), None);
        assert_eq!(nodes.len(), N);
        for node in &nodes {
            assert_eq!(node.role, AriaRole::Button);
            assert_eq!(node.expanded, Some(false));
            assert_eq!(node.state.checked, None, "a disclosure carries no aria-checked");
        }
    }

    #[test]
    fn expanded_section_sets_aria_expanded_true() {
        let nodes = AccordionSingleView::access_node(&with_expanded(2), None);
        assert_eq!(nodes[2].expanded, Some(true));
        assert_eq!(nodes[0].expanded, Some(false));
        assert_eq!(nodes[1].expanded, Some(false));
    }

    #[test]
    fn focused_header_marks_only_that_node_focused() {
        let nodes = AccordionSingleView::access_node(&idle(), Some(ROW_TAGS[1]));
        assert!(!nodes[0].state.focused);
        assert!(nodes[1].state.focused);
        assert!(!nodes[2].state.focused);
    }

    #[test]
    fn names_come_from_each_section_summary() {
        let state = idle();
        let scene = pinion_core::Owner::new().run(|| view(&state, &Frame::new()));
        let mut nodes = AccordionSingleView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        for (i, node) in nodes.iter().enumerate() {
            assert_eq!(node.name.as_deref(), Some(SUMMARIES[i]));
        }
    }

    // ── access_child_invoke (AT activation) ───────────────────────

    #[test]
    fn access_child_invoke_click_expands_addressed_section() {
        let mut scene = scene_fixture();
        assert!(AccordionSingleView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "1",
            AccessAction::Click,
        ));
        assert_eq!(expanded_index(&scene), Some(1));
    }

    #[test]
    fn access_child_invoke_click_enforces_single_open() {
        let mut scene = scene_fixture();
        AccordionSingleView::access_child_invoke(&mut scene, PRIMARY_TAG, "0", AccessAction::Click);
        assert_eq!(expanded_index(&scene), Some(0));
        AccordionSingleView::access_child_invoke(&mut scene, PRIMARY_TAG, "2", AccessAction::Click);
        assert_eq!(expanded_index(&scene), Some(2));
        assert!(!AccordionSingleView::read_state(&scene)[0].1, "AT click enforces single-open");
    }

    #[test]
    fn access_child_invoke_focus_returns_true_without_mutation() {
        let mut scene = scene_fixture();
        AccordionSingleView::access_child_invoke(&mut scene, PRIMARY_TAG, "1", AccessAction::Click);
        let before = expanded_index(&scene);
        assert!(AccordionSingleView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "0",
            AccessAction::Focus,
        ));
        assert_eq!(expanded_index(&scene), before, "Focus does not toggle expansion");
    }

    #[test]
    fn access_child_invoke_out_of_range_and_non_numeric_decline() {
        let mut scene = scene_fixture();
        assert!(!AccordionSingleView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "9",
            AccessAction::Click,
        ));
        assert!(!AccordionSingleView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "foo",
            AccessAction::Click,
        ));
        assert_eq!(expanded_index(&scene), None);
    }
}
