// R690 §5.16 — example bindings tolerate looser doc-markdown lints
// than substrate crates; the architectural narrative carries many
// proper-noun identifiers (TabList, RadioGroup, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-tabs` — R690 §5.16 §5.40 §5.50 first consumer of the
//! `pinion_widget_paint::tabs` substrate.
//!
//! ## Why this binding exists
//!
//! Phase B widget-catalog entry. A horizontal Material 3 tab strip
//! (the [`view_tabs`] substrate) over a per-tab content panel — the
//! canonical DCC / IDE / settings-dialog tabbed layout that composes
//! with the dock editor toward the northern-star "engine-class editor
//! self-hosted in pinion".
//!
//! ## Selection model is reused, not reinvented
//!
//! "Select 1 of N" is exactly [`RadioGroup`] semantics, so the tab
//! selection runs on a reused
//! [`RadioGroupExternal`]
//! — no new SCXML statechart (per [[abstraction-needs-second-consumer]];
//! the settings-panel nav rail set the same precedent). The composite
//! tags `tabs#<i>` route cursor hits through the input router's `#`
//! split (R51.42) into `invoke("send", "<i>:<EventName>")` against the
//! single composite handle, exactly like `hello-radio-group`. The only
//! divergence is the AT role triple (`tablist` / `tab` / `tabpanel`)
//! and the horizontal keyboard model.
//!
//! ## Keyboard model (WAI-ARIA 1.2 §3.6 Tabs, automatic activation)
//!
//! The TabList is one tab stop; Arrow keys move *and* select the
//! adjacent tab (automatic activation — the WAI-ARIA default that maps
//! one-to-one onto RadioGroup's "Arrow activates" model):
//!
//! - Arrow Right — select the next tab (wraps at the end).
//! - Arrow Left — select the previous tab (wraps at the start).
//! - Home — select the first tab.
//! - End — select the last tab.
//!
//! Selection runs the full `PointerEnter → Down → Up → Leave` cycle
//! through the composite's `send` wire format so RPC headless / human
//! cursor / keyboard all converge on the same statechart path
//! (§2 invariant #2).
//!
//! ## AI clients
//!
//! Reach the strip through the same introspect surface:
//! `query("selected_index")` and `invoke("send", "<i>:<EventName>")`.
//! The active panel content is queryable via `scene/snapshot` on the
//! `tabs_panel` node (§2 invariant #7 — scene-as-data).
//!
//! [`RadioGroup`]: pinion_core::widgets::radio_group::RadioGroup

use pinion_a11y::{AccessAction, AccessFocus, AccessNode, TabCell, WidgetA11y, tablist_tab_nodes};
// R815 §5.40 — `AriaRole` is now only referenced by the test asserts (the
// lifted `tablist_tab_nodes` builder owns the role + state tagging in prod).
#[cfg(test)]
use pinion_a11y::AriaRole;
use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::tabs::{TabsStyle, composite_tab_tag, view_tabs};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTabsRenderer, HelloTabsRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 320;
const THEME_TAG: &str = "app";
const N: usize = 3;
const PRIMARY_TAG: &str = "tabs";
const PANEL_TAG: &str = "tabs_panel";
const HEADER_FONT_PX: u32 = 13;
const PANEL_FONT_PX: u32 = 16;
const FOOTER_FONT_PX: u32 = 12;

/// Visible tab labels. Index `i` becomes the tab tagged `tabs#<i>`;
/// the label is also each tab's WAI-ARIA name-from-contents.
const TAB_LABELS: [&str; N] = ["General", "Appearance", "Advanced"];

/// Per-tab panel body shown when that tab is active. Each carries a
/// distinct lead word so RPC drivers can verify the panel swaps with
/// the selection.
const TAB_CONTENT: [&str; N] = [
    "Language, startup behaviour, and default actions.",
    "Theme, accent colour, and text scale.",
    "Developer options, telemetry, and resets.",
];

/// Cached projection of the tab strip: the active tab + the WAI-ARIA
/// roving-tabindex active descendant. `Copy` because both fields are
/// `Copy`, so the shell hands the snapshot into the paint closure
/// without lifetime gymnastics.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
struct TabsState {
    /// Currently selected tab index (the panel shown). `None` only at
    /// the pre-seed boundary; `create_external` seeds `Some(0)` so a
    /// panel always renders.
    selected: Option<usize>,
    /// R51.87 §5.40 — AT-side active descendant (roving tabindex).
    /// `None` falls back to `selected`-or-0 at `access_focus_target`.
    focused: Option<usize>,
}

/// view-fn (§6.3): pure sync mapping `TabsState -> Scene`. The
/// [`view_tabs`] substrate paints the strip (tagged `tabs`, per-tab
/// `tabs#<i>`); a sibling panel container (tagged `tabs_panel`,
/// AT role `tabpanel`) shows the active tab's body.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: TabsState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let strip = view_tabs(
        PRIMARY_TAG,
        &TAB_LABELS,
        state.selected,
        &theme,
        &TabsStyle::m3_default(),
    );

    let active = state.selected.unwrap_or(0);
    let panel = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            TAB_CONTENT[active],
            Rect::default(),
            TextStyle::new()
                .with_size_px(PANEL_FONT_PX)
                .with_fg(on_surface),
        ))])
        .with_tag(PANEL_TAG)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_justify(JustifyContent::Start)
                .with_flex_grow(1.0)
                .with_padding(Rect::new(16, 16, 16, 16)),
        ),
    );

    let footer = Scene::Text(TextNode::styled(
        "\u{2190}/\u{2192} switch tab   Home/End jump",
        Rect::default(),
        TextStyle::new()
            .with_size_px(FOOTER_FONT_PX)
            .with_fg(on_surface_muted),
    ));
    let header = Scene::Text(TextNode::styled(
        "Settings",
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(on_surface_muted),
    ));

    Scene::Container(
        ContainerNode::new(vec![header, strip, panel, footer])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Stretch)
                    .with_gap(8)
                    .with_padding(Rect::new(12, 12, 12, 12)),
            ),
    )
}

struct TabsView;

impl WidgetCore for TabsView {
    type State = TabsState;
    // Every state change flows through `apply_key` (wire format
    // `"<i>:<EventName>"`) + the InputRouter sub-index dispatch, so no
    // keybinding-channel events flow through `event_name`. `()` (via
    // the radio-group precedent) keeps the trait's `Copy` bound
    // satisfied without an unused variant.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // ★ R1703 §5.45 — a tab STRIP takes the wheel; a form's radio set does
        // not, and one type serves both, so the role is declared here rather
        // than guessed from the shape. Measured on the reference toolkit's tab
        // bar by probe: a wheel down at tab 0 lands on tab 1 and up returns.
        let mut group = RadioGroupExternal::new(N).with_wheel(true);
        // Form default: tab 0 active (a tab strip always shows one
        // panel). `intervene` is the slot-assignment path — it sets
        // `selected_index` without firing the `selected` intent and
        // without touching `focused_index`, so the AT active descendant
        // stays unset until the user actually navigates.
        let _ = group.intervene("selected_index", IntrospectValue::Int(0));
        Box::new(group)
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> TabsState {
        let mut out = TabsState::default();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        out.selected = match intro.query("selected_index") {
            Ok(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
            _ => None,
        };
        out.focused = match intro.query("focused_index") {
            Ok(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
            _ => None,
        };
        out
    }

    fn view(state: TabsState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-tabs (R690 §5.16 §5.40 §5.50)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        // Single source of truth for the keymap lives in `apply_key`.
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // WAI-ARIA tabs roving tabindex: the TabList is one tab stop,
        // so Arrow / Home / End route only when the strip itself owns
        // focus. Sibling controls keep their own Arrow keys.
        if focused != Some(Self::tag()) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(idx) = resolve_target_index(node.handle.introspect(), key) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        // Automatic activation: the full PointerEnter / Down / Up /
        // Leave cycle runs against the target tab. PointerUp is the
        // activation edge (`RadioGroup::send` enforces mutual exclusion
        // + fires the `selected` intent + syncs `focused_index` per
        // R51.90); the trailing Leave returns the row's interaction
        // state to Idle.
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            let _ = intro.invoke("send", IntrospectValue::Text(format!("{idx}:{ev}")));
        }
        true
    }

    fn fmt_state_log(state: &TabsState) -> String {
        let sel = state
            .selected
            .map_or_else(|| "-".to_string(), |i| i.to_string());
        match state.focused {
            Some(f) => format!("selected={sel} focused={f}"),
            None => format!("selected={sel}"),
        }
    }
}

impl WidgetA11y for TabsView {
    /// R690 §5.40 — WAI-ARIA 1.2 §3.6 tablist/tab/tabpanel tree.
    /// Emits N+2 nodes: one `AriaRole::TabList` parent claiming each
    /// tab as a child, one `AriaRole::Tab` per index carrying
    /// `aria-selected` + `aria-posinset`/`aria-setsize`, and one
    /// `AriaRole::TabPanel` for the active tab's content region.
    ///
    /// Per-tab accessible names are derived by `enrich_names_from_scene`
    /// from each tab's label `TextNode`. The TabList name and the
    /// TabPanel name (= the active tab's label, the WAI-ARIA
    /// labelled-by-its-tab convention) have no single paint-scene
    /// equivalent, so they stay explicit overrides.
    fn access_node(state: &TabsState, focused: Option<&str>) -> Vec<AccessNode> {
        // R815 §5.40 — lifted `tablist_tab_nodes` builder (one of two
        // consumers). `label: None` — tab names come from
        // `enrich_names_from_scene` (the painted tab text); the builder
        // derives `aria-posinset` / `aria-setsize` from the slice.
        let group_focused = focused == Some(<Self as WidgetCore>::tag());
        let active = active_tab_index(*state);
        let tab_tags: Vec<String> = (0..N).map(|i| composite_tab_tag(PRIMARY_TAG, i)).collect();
        let tabs: Vec<TabCell<'_>> = (0..N)
            .map(|i| TabCell {
                tag: &tab_tags[i],
                label: None,
                selected: state.selected == Some(i),
                focused: group_focused && i == active,
            })
            .collect();
        tablist_tab_nodes(
            <Self as WidgetCore>::tag(),
            "Settings sections",
            &tabs,
            PANEL_TAG,
            // (R1320) These labels are static, so name the panel explicitly; a caller
            // whose labels are runtime app state passes `None` and is labelled by its
            // tab (the dock's display titles).
            Some(TAB_LABELS[active]),
        )
    }

    /// R51.71 §5.40 — composite focus model. When the TabList itself is
    /// focused, focus stays on the parent (`tabs`) and the active tab's
    /// sub-tag (`tabs#<i>`) is reported as `aria-activedescendant`.
    fn access_focus_target(state: &TabsState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(<Self as WidgetCore>::tag()) {
            let idx = active_tab_index(*state);
            Some(AccessFocus::composite(
                <Self as WidgetCore>::tag(),
                composite_tab_tag(PRIMARY_TAG, idx),
            ))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// R51.70 §5.40 — composite child action dispatch. `Click` /
    /// `Default` on a tab's NodeId (`tabs#<i>`) activate it (mutual
    /// exclusion + `selected` intent). `Focus` records the AT-side
    /// active descendant without committing a selection.
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
            AccessAction::Focus => {
                if let Ok(i) = i64::try_from(idx) {
                    let _ = intro.intervene("focused_index", IntrospectValue::Int(i));
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for TabsView {
    type Renderer = HelloTabsRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Resolve a keyboard key to a target tab index given the strip's
/// current `selected_index`. Home/End jump to the first/last tab;
/// Arrow Right/Left step by one with wrap-around (the WAI-ARIA tabs
/// cyclic convention). Returns `None` for unrecognised keys so the
/// shell's swallow path matches the unrecognised-keybinding contract.
fn resolve_target_index(intro: Option<&dyn ExternalIntrospect>, key: &str) -> Option<usize> {
    match key {
        "Home" => Some(0),
        "End" => Some(N - 1),
        "ArrowRight" => Some(arrow_step(intro, 1)),
        "ArrowLeft" => Some(arrow_step(intro, -1)),
        _ => None,
    }
}

/// Next target index for an arrow step. `direction` is `+1`
/// (ArrowRight) or `-1` (ArrowLeft); wraps at the ends. With no tab
/// selected, ArrowRight lands on `0` and ArrowLeft on `N - 1`.
fn arrow_step(intro: Option<&dyn ExternalIntrospect>, direction: i32) -> usize {
    let current: Option<usize> = intro
        .and_then(|i| i.query("selected_index").ok())
        .and_then(|v| match v {
            IntrospectValue::Int(i) => usize::try_from(i).ok(),
            _ => None,
        });
    match (current, direction) {
        (Some(c), 1) => (c + 1) % N,
        (Some(c), -1) => (c + N - 1) % N,
        (None, 1) => 0,
        (None, -1) => N - 1,
        _ => 0,
    }
}

/// R51.66 §5.40 — active tab index (the row reported as the AT-side
/// active descendant). Resolution: AT-pinned `focused` first, then the
/// selected tab, then `0`.
fn active_tab_index(state: TabsState) -> usize {
    if let Some(idx) = state.focused {
        return idx;
    }
    state.selected.unwrap_or(0)
}

fn main() {
    pinion_shell::run::<TabsView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn selected_state(idx: usize) -> TabsState {
        TabsState {
            selected: Some(idx),
            focused: None,
        }
    }

    #[test]
    fn r690_emits_tablist_plus_n_tabs_plus_panel() {
        let nodes = TabsView::access_node(&selected_state(0), None);
        assert_eq!(nodes.len(), N + 2);
        assert_eq!(nodes[0].role, AriaRole::TabList);
        assert_eq!(nodes[0].name.as_deref(), Some("Settings sections"));
        for i in 0..N {
            assert_eq!(nodes[i + 1].role, AriaRole::Tab);
        }
        assert_eq!(nodes[N + 1].role, AriaRole::TabPanel);
    }

    #[test]
    fn r690_tablist_lists_all_tabs_in_order() {
        let nodes = TabsView::access_node(&selected_state(0), None);
        assert_eq!(nodes[0].children.len(), N);
        for i in 0..N {
            assert_eq!(nodes[0].children[i], format!("tabs#{i}"));
        }
    }

    #[test]
    fn r690_selected_tab_carries_aria_selected_true_others_false() {
        let nodes = TabsView::access_node(&selected_state(1), None);
        assert_eq!(nodes[1].selected, Some(false));
        assert_eq!(nodes[2].selected, Some(true));
        assert_eq!(nodes[3].selected, Some(false));
    }

    #[test]
    fn r690_tabs_carry_posinset_and_setsize() {
        let nodes = TabsView::access_node(&selected_state(0), None);
        for i in 0..N {
            assert_eq!(
                nodes[i + 1].position_in_set,
                Some(u32::try_from(i + 1).unwrap())
            );
            assert_eq!(nodes[i + 1].size_of_set, Some(u32::try_from(N).unwrap()));
        }
    }

    #[test]
    fn r690_panel_name_is_active_tab_label() {
        let nodes = TabsView::access_node(&selected_state(2), None);
        assert_eq!(nodes[N + 1].name.as_deref(), Some("Advanced"));
    }

    #[test]
    fn r690_each_tab_label_enriched_from_paint() {
        let state = selected_state(0);
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = TabsView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[1].name.as_deref(), Some("General"));
        assert_eq!(nodes[2].name.as_deref(), Some("Appearance"));
        assert_eq!(nodes[3].name.as_deref(), Some("Advanced"));
    }

    #[test]
    fn r690_tablist_explicit_name_survives_enrichment() {
        let state = selected_state(0);
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = TabsView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[0].name.as_deref(), Some("Settings sections"));
    }

    #[test]
    fn r690_focused_tablist_marks_active_tab_focused() {
        let nodes = TabsView::access_node(&selected_state(1), Some("tabs"));
        assert!(nodes[2].state.focused);
        assert!(!nodes[1].state.focused);
        assert!(!nodes[3].state.focused);
    }

    #[test]
    fn r690_unfocused_tablist_no_tab_focused() {
        let nodes = TabsView::access_node(&selected_state(1), None);
        for tab in nodes.iter().skip(1).take(N) {
            assert!(!tab.state.focused);
        }
    }

    #[test]
    fn r690_access_focus_target_parent_focus_with_active_tab() {
        let target = TabsView::access_focus_target(&selected_state(2), Some("tabs"))
            .expect("focused TabList returns Some");
        assert_eq!(target.focus_tag, "tabs");
        assert_eq!(target.active_descendant.as_deref(), Some("tabs#2"));
    }

    #[test]
    fn r690_access_focus_target_none_when_no_focus() {
        assert!(TabsView::access_focus_target(&selected_state(0), None).is_none());
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn scene() -> Scene {
        // R1703 — `with_wheel` matches `create_external` so this fixture is the
        // production widget: a fixture that differs from what ships is how a
        // test comes to pass against a screen nobody has.
        let mut ext = RadioGroupExternal::new(N).with_wheel(true);
        let _ = ext.intervene("selected_index", IntrospectValue::Int(0));
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
    fn r690_arrow_right_advances_selection() {
        let mut s = scene();
        assert!(TabsView::apply_key(
            &mut s,
            Some("tabs"),
            "ArrowRight",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(1));
    }

    #[test]
    fn r690_arrow_left_wraps_to_last() {
        let mut s = scene();
        assert!(TabsView::apply_key(
            &mut s,
            Some("tabs"),
            "ArrowLeft",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(i64::try_from(N - 1).unwrap()));
    }

    #[test]
    fn r690_end_jumps_to_last_home_to_first() {
        let mut s = scene();
        assert!(TabsView::apply_key(
            &mut s,
            Some("tabs"),
            "End",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(i64::try_from(N - 1).unwrap()));
        assert!(TabsView::apply_key(
            &mut s,
            Some("tabs"),
            "Home",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(0));
    }

    #[test]
    fn r690_unfocused_strip_swallows_arrow() {
        let mut s = scene();
        assert!(!TabsView::apply_key(
            &mut s,
            Some("other_widget"),
            "ArrowRight",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(selected_index(&s), Some(0));
    }

    #[test]
    fn r690_access_child_invoke_click_selects_tab() {
        let mut s = scene();
        assert!(TabsView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "2",
            AccessAction::Click,
        ));
        assert_eq!(selected_index(&s), Some(2));
    }

    #[test]
    fn r690_access_child_invoke_out_of_range_declines() {
        let mut s = scene();
        assert!(!TabsView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "9",
            AccessAction::Click,
        ));
    }

    #[test]
    fn r690_view_contains_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TabsView>(
            TabsState::default(),
            &Frame::default(),
        );
    }
}
