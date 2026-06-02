//! `hello-accordion` — R697 §5.38 §5.50 multi-open WAI-ARIA APG
//! **accordion** on the pinion-shell substrate.
//!
//! An accordion is a vertically stacked set of [`Disclosure`] sections:
//! each section is a header `button` carrying `aria-expanded` that
//! reveals or hides its own content panel. Per the WAI-ARIA APG
//! "Accordion" pattern the default is **multi-open** — more than one
//! panel may be expanded at once — so each section is an independent
//! disclosure with no cross-section coordination. (Single-open
//! "exclusive" accordions are an opt-in variant; that coordination is
//! framework-owned mutual exclusion in the `RadioGroup` mould and waits
//! for a 2nd consumer per `[[abstraction-needs-second-consumer]]`.)
//!
//! This binary is the **2nd consumer** of the R696 Disclosure
//! substrate: N=3 [`DisclosureExternal`]s are composed through the
//! R55.D.5 [`WidgetCore::create_extra_externals`] slot (the same
//! homogeneous-cluster path `examples/settings-panel` uses for its six
//! `CheckboxExternal`s), each header tagged `accordion_sec_{i}`. The
//! `InputRouter`'s depth-first walk over the `Scene::Container` root
//! routes a click on header `i` to that section's handle, so the three
//! disclosures toggle independently with zero new substrate.
//!
//! Keyboard model (WAI-ARIA APG accordion):
//! - each header is its **own Tab stop** ([`focusable_tags`] enumerates
//!   all three — unlike a `RadioGroup`, which is a single tab stop with
//!   internal roving), so `Tab` / `Shift+Tab` move between sections;
//! - `Space` / `Enter` toggle the focused section (the disclosure
//!   keyboard model — both keys, not just `Space`);
//! - `ArrowDown` / `ArrowUp` move focus to the next / previous header
//!   (wrapping), and `Home` / `End` jump to the first / last — the
//!   optional APG arrow-navigation, routed through the R664
//!   [`pinion_core::focus_request`] mailbox so the move funnels through
//!   the same `FocusManager::focus_set` path a mouse press would.
//!
//! Visual focus indication on the header rows reuses the shared
//! `view_disclosure` paint and therefore inherits the R694 focus-ring
//! carry (composite paint widgets do not yet draw a ring); the focus
//! model is nonetheless fully AT-reported (`access_node` marks the
//! focused header) and RPC-verifiable (`focus/get`).

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::disclosure::{DisclosureExternal, DisclosureState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::disclosure::{view_disclosure, DisclosureStyle};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloAccordionRenderer, HelloAccordionRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 440;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across
/// the example gallery.
const THEME_TAG: &str = "app";

/// Section count. Three is enough to exercise wrap-around arrow
/// roving (last → first) and to read as a real settings accordion.
const N: usize = 3;

/// Per-section header dispatch tags. `&'static str` (not `format!`)
/// because [`WidgetCore::focusable_tags`] returns `Vec<&'static str>`
/// — the same fixed-cluster convention as `settings-panel`'s
/// `NOTIF_INSTANCE_TAGS`. Each tag lands on one section header via
/// [`view_disclosure`], so the input router hit-tests a click on
/// header `i` straight to `accordion_sec_{i}`.
const SECTION_TAGS: [&str; N] =
    ["accordion_sec_0", "accordion_sec_1", "accordion_sec_2"];

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
    ["accordion_body_0", "accordion_body_1", "accordion_body_2"];

/// Cached projection: one `(DisclosureState, expanded)` pair per
/// section, read from each [`DisclosureExternal`]'s introspect slots.
/// `Copy` because both inner types are `Copy`, so the shell hands the
/// snapshot into the paint closure without lifetime gymnastics.
type AccordionState = [(DisclosureState, bool); N];

/// view-fn (§6.3): pure sync mapping [`AccordionState`] → [`Scene`].
/// A vertical column of N [`view_disclosure`] sections, each tagged
/// `accordion_sec_{i}` on its header (the click target) with a
/// tagged panel body shown only while expanded.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &AccordionState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = DisclosureStyle::m3();
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
                SECTION_TAGS[i],
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
        ContainerNode::new(sections).with_layout(
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

/// Read one section's `(state, expanded)` from the live scene via the
/// §5.15 introspect channel — the same path an RPC
/// `scene/query /accordion_sec_{i}/external/expanded` request walks, so
/// the cached projection and the AI client never diverge.
fn read_section(scene: &Scene, tag: &str) -> (DisclosureState, bool) {
    let Some(node) = scene.find_external_with_tag(tag) else {
        return (DisclosureState::Idle, false);
    };
    let Some(intro) = node.handle.introspect() else {
        return (DisclosureState::Idle, false);
    };
    let state = match intro.query("state") {
        Some(IntrospectValue::Text(name)) => DisclosureState::from_name_or_default(&name),
        _ => DisclosureState::Idle,
    };
    let expanded = matches!(intro.query("expanded"), Some(IntrospectValue::Bool(true)));
    (state, expanded)
}

pub struct AccordionView;

impl WidgetCore for AccordionView {
    type State = AccordionState;
    // Every state change flows through `apply_key` (keyboard) or the
    // InputRouter's per-section `"send"` dispatch (pointer), never the
    // shell's enum-typed `keybinding` channel — so `()` satisfies the
    // trait's `Copy` bound without an unused event variant (mirror of
    // `hello-radio-group`).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(DisclosureExternal::new())
    }

    /// Sections 1..N as sibling externals; section 0 is the primary
    /// [`Self::create_external`]. The substrate wraps the root as
    /// `Scene::Container([primary, ...extras])`, and the input router's
    /// depth-first walk dispatches each header click by tag.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        SECTION_TAGS
            .iter()
            .skip(1)
            .map(|tag| ExtraExternal::new(*tag, Box::new(DisclosureExternal::new())))
            .collect()
    }

    fn tag() -> &'static str {
        SECTION_TAGS[0]
    }

    fn read_state(scene: &Scene) -> AccordionState {
        let mut out: AccordionState = [(DisclosureState::Idle, false); N];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = read_section(scene, SECTION_TAGS[i]);
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
        "pinion hello-accordion (R697 §5.38 multi-open APG accordion)"
    }

    /// Each section header is its own Tab stop (WAI-ARIA APG: an
    /// accordion header is a `button` in the document focus order),
    /// unlike a `RadioGroup`'s single-tab-stop roving model.
    fn focusable_tags() -> Vec<&'static str> {
        SECTION_TAGS.to_vec()
    }

    /// WAI-ARIA APG accordion keyboard model, gated on the focused
    /// header (roving-tabindex `apply_key` discipline — keys route only
    /// when one of our headers owns focus):
    ///
    /// - `Space` / `Enter` — toggle the focused section (both keys, the
    ///   disclosure model);
    /// - `ArrowDown` / `ArrowUp` — move focus to the next / previous
    ///   header, wrapping at the ends;
    /// - `Home` / `End` — move focus to the first / last header.
    ///
    /// Focus moves are requested through the R664
    /// [`pinion_core::focus_request`] mailbox; the shell drains it after
    /// this dispatch and applies it via the same `FocusManager` path a
    /// mouse press uses, so `External::on_focus_change` observers fire
    /// identically.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let Some(focused_tag) = focused else {
            return false;
        };
        if matches!(key, "Space" | "Enter") {
            let Some(node) = scene.find_external_with_tag_mut(focused_tag) else {
                return false;
            };
            let Some(intro) = node.handle.introspect_mut() else {
                return false;
            };
            return intro
                .invoke("send", IntrospectValue::Text("KeyboardActivate".to_string()))
                .is_ok();
        }
        // R757 §5.39 — the WAI-ARIA roving-tabindex navigation (next /
        // previous / first / last, wrapping) is the shared
        // [`pinion_core::focus_request::rove`] SSOT; only the disclosure
        // *activation* above stays per-binding.
        pinion_core::focus_request::rove(SECTION_TAGS.as_slice(), focused_tag, key)
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

impl WidgetA11y for AccordionView {
    /// R697 §5.40 — flat contribution of N `button` nodes, one per
    /// section header, mirroring the R696 single-disclosure
    /// `access_node` per section. Each carries `aria-expanded` (the
    /// R696 `AccessNode::with_expanded` axis), the interaction state
    /// flags, and `focused` when that header owns shell focus. No
    /// parent container node — WAI-ARIA defines no "accordion" role;
    /// the section headers are plain heading/button controls in the
    /// document order (here each header is a Tab stop in its own
    /// right), so the accessible names come from name-from-contents
    /// enrichment of each header's summary label.
    fn access_node(state: &AccordionState, focused: Option<&str>) -> Vec<AccessNode> {
        (0..N)
            .map(|i| {
                let (interaction, expanded) = state[i];
                AccessNode::new(SECTION_TAGS[i], AriaRole::Button)
                    .with_expanded(expanded)
                    .with_state(AccessState {
                        focused: focused == Some(SECTION_TAGS[i]),
                        ..AccessState::from_interaction(interaction, None)
                    })
            })
            .collect()
    }
}

impl WidgetView for AccordionView {
    type Renderer = HelloAccordionRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<AccordionView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> AccordionState {
        [(DisclosureState::Idle, false); N]
    }

    fn with_expanded(idx: usize) -> AccordionState {
        let mut s = idle();
        s[idx].1 = true;
        s
    }

    // ── view / composition ────────────────────────────────────────

    #[test]
    fn view_carries_primary_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<AccordionView>(
            idle(),
            &Frame::new(),
        );
    }

    #[test]
    fn view_carries_every_section_header_tag() {
        let scene = pinion_core::Owner::new().run(|| view(&idle(), &Frame::new()));
        for tag in SECTION_TAGS {
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

    #[test]
    fn multi_open_independent_sections() {
        // Sections 0 and 2 expanded simultaneously (multi-open APG
        // default): both bodies present at once.
        let mut s = idle();
        s[0].1 = true;
        s[2].1 = true;
        let scene = pinion_core::Owner::new().run(|| view(&s, &Frame::new()));
        assert!(scene.contains_tag(BODY_TAGS[0]));
        assert!(!scene.contains_tag(BODY_TAGS[1]));
        assert!(scene.contains_tag(BODY_TAGS[2]));
    }

    // ── focus model ───────────────────────────────────────────────

    #[test]
    fn every_header_is_a_tab_stop() {
        assert_eq!(AccordionView::focusable_tags(), SECTION_TAGS.to_vec());
    }

    #[test]
    fn arrow_down_requests_next_header_wrapping() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain(); // clear any prior
        assert!(AccordionView::apply_key(
            &mut scene,
            Some(SECTION_TAGS[N - 1]),
            "ArrowDown",
            pinion_core::Modifiers::empty(),
        ));
        // Wraps from the last header back to the first.
        assert_eq!(
            pinion_core::focus_request::drain().as_deref(),
            Some(SECTION_TAGS[0]),
        );
    }

    #[test]
    fn arrow_up_requests_previous_header_wrapping() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(AccordionView::apply_key(
            &mut scene,
            Some(SECTION_TAGS[0]),
            "ArrowUp",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(
            pinion_core::focus_request::drain().as_deref(),
            Some(SECTION_TAGS[N - 1]),
        );
    }

    #[test]
    fn home_and_end_jump_to_first_and_last() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(AccordionView::apply_key(
            &mut scene,
            Some(SECTION_TAGS[1]),
            "Home",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(SECTION_TAGS[0]));
        assert!(AccordionView::apply_key(
            &mut scene,
            Some(SECTION_TAGS[1]),
            "End",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain().as_deref(), Some(SECTION_TAGS[N - 1]));
    }

    #[test]
    fn arrow_keys_ignored_when_no_header_focused() {
        let mut scene = scene_fixture();
        let _ = pinion_core::focus_request::drain();
        assert!(!AccordionView::apply_key(
            &mut scene,
            None,
            "ArrowDown",
            pinion_core::Modifiers::empty(),
        ));
        assert!(!AccordionView::apply_key(
            &mut scene,
            Some("some_sibling"),
            "ArrowDown",
            pinion_core::Modifiers::empty(),
        ));
        assert_eq!(pinion_core::focus_request::drain(), None, "no focus request emitted");
    }

    #[test]
    fn space_toggles_focused_section_only() {
        let mut scene = scene_fixture();
        assert!(AccordionView::apply_key(
            &mut scene,
            Some(SECTION_TAGS[1]),
            "Space",
            pinion_core::Modifiers::empty(),
        ));
        let s = AccordionView::read_state(&scene);
        assert!(s[1].1, "focused section 1 expands");
        assert!(!s[0].1, "section 0 untouched");
        assert!(!s[2].1, "section 2 untouched");
    }

    #[test]
    fn enter_also_toggles_focused_section() {
        let mut scene = scene_fixture();
        assert!(AccordionView::apply_key(
            &mut scene,
            Some(SECTION_TAGS[0]),
            "Enter",
            pinion_core::Modifiers::empty(),
        ));
        assert!(AccordionView::read_state(&scene)[0].1, "Enter expands section 0");
    }

    // ── a11y ──────────────────────────────────────────────────────

    #[test]
    fn emits_n_button_nodes_with_aria_expanded() {
        let nodes = AccordionView::access_node(&idle(), None);
        assert_eq!(nodes.len(), N);
        for node in &nodes {
            assert_eq!(node.role, AriaRole::Button);
            assert_eq!(node.expanded, Some(false));
            assert_eq!(node.state.checked, None, "a disclosure carries no aria-checked");
        }
    }

    #[test]
    fn expanded_section_sets_aria_expanded_true() {
        let nodes = AccordionView::access_node(&with_expanded(2), None);
        assert_eq!(nodes[2].expanded, Some(true));
        assert_eq!(nodes[0].expanded, Some(false));
        assert_eq!(nodes[1].expanded, Some(false));
    }

    #[test]
    fn focused_header_marks_only_that_node_focused() {
        let nodes = AccordionView::access_node(&idle(), Some(SECTION_TAGS[1]));
        assert!(!nodes[0].state.focused);
        assert!(nodes[1].state.focused);
        assert!(!nodes[2].state.focused);
    }

    #[test]
    fn names_come_from_each_section_summary() {
        let state = idle();
        let scene = pinion_core::Owner::new().run(|| view(&state, &Frame::new()));
        let mut nodes = AccordionView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        for (i, node) in nodes.iter().enumerate() {
            assert_eq!(node.name.as_deref(), Some(SUMMARIES[i]));
        }
    }

    /// Build a fresh accordion scene mirroring the shell's boot
    /// composition (`Scene::Container([primary, ...extras])`) so
    /// `apply_key` walks the live topology.
    fn scene_fixture() -> Scene {
        use pinion_core::scene::ExternalNode;
        let primary = Scene::External(
            ExternalNode::new(AccordionView::create_external()).with_tag(SECTION_TAGS[0]),
        );
        let mut children = vec![primary];
        for extra in AccordionView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag.into_owned()),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
