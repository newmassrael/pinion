//! `hello-group-box` — R1554 §5.39 §5.50 §5.40: the toolkit group box, and the
//! **inherited-disabled** scene declaration it is the first consumer of.
//!
//! Three real checkboxes. The first is the group's title checkbox — the
//! toolkit's `setCheckable(true)` gate — and the other two live inside the group's content
//! region. Clearing the gate puts ONE flag
//! ([`LayoutStyle::with_disabled`](pinion_core::style::LayoutStyle::with_disabled)) on that region, and
//! everything else follows from the §5.39 cascade with no further code in this
//! binding:
//!
//! * the two inner checkboxes leave the Tab order,
//! * a click aimed at either of them resolves to the region instead,
//! * both are announced `aria-disabled` to a screen reader and on
//!   `scene/access`,
//! * their ink fades by the Material 3 disabled token — the same ink a
//!   self-disabled checkbox gets from its own state layer,
//! * `scene/disabled` answers **which** node disabled them, which the toolkit has no
//!   accessor for at all,
//! * `focus/set` on either answers `tag_disabled` naming the gate's region,
//!   where the toolkit's `setFocus()` is a silent no-op.
//!
//! What this binding does NOT contain is the interesting part: no per-widget
//! disabled bookkeeping, no Tab-order list, no `aria-disabled` plumbing, and no
//! second copy of the fade. The gate is `checked`; the rest is derived.
//!
//! Keyboard: <kbd>Space</kbd> toggles the focused checkbox; <kbd>Alt</kbd>+`A`
//! reaches the group through the `&` in its title (R1543), exactly as the
//! toolkit's group box title mnemonic does.

use pinion_a11y::{AccessFocus, AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::checkbox::{CheckboxExternal, CheckboxState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::checkbox::{CheckboxStyle, view_checkbox};
use pinion_widget_paint::group_box::{GroupBoxCheck, GroupBoxStyle, view_group_box};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGroupBoxRenderer, HelloGroupBoxRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 260;
const THEME_TAG: &str = "app";

/// The group's own tag. Its title and content tags are derived from it by
/// [`GroupBoxTag`](pinion_core::composite_tag::GroupBoxTag) — see
/// [`GATE_TAG`] / [`REGION_TAG`], which a test pins against that SSOT rather
/// than trusting the spelling.
const GROUP_TAG: &str = "advanced";
/// The gate checkbox. It IS the group's title band, so the tag must be the one
/// `view_group_box` paints — `GroupBoxTag::title(GROUP_TAG)`.
const GATE_TAG: &str = "advanced_title";
/// The content region — what `scene/disabled` names as the cause.
const REGION_TAG: &str = "advanced_content";

/// The two checkboxes inside the group.
const INNER_TAGS: [&str; 2] = ["opt_verbose", "opt_trace"];
const INNER_LABELS: [&str; 2] = ["Verbose logging", "Trace every frame"];

/// Gate first, then the two inner checkboxes — index 0 is the primary external.
const N: usize = 3;
type States = [(CheckboxState, bool); N];

/// view-fn (§6.3): pure sync `States -> Scene`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &States, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let (gate_interaction, gated_on) = state[0];

    let inner: Vec<Scene> = INNER_TAGS
        .iter()
        .zip(INNER_LABELS)
        .enumerate()
        .map(|(i, (tag, label))| {
            let (interaction, checked) = state[i + 1];
            let row = view_checkbox(
                tag,
                interaction,
                checked,
                &theme,
                &CheckboxStyle::m3_filled(),
                label,
            );
            // A Tab stop while the group is live — and the cascade takes it out
            // of the enumeration when the region is disabled, without this
            // binding saying anything about it.
            with_focus_stop(row)
        })
        .collect();

    let group = view_group_box(
        GROUP_TAG,
        // The toolkit accepts the same `&` marker in a group-box title.
        "&Advanced",
        Some(GroupBoxCheck {
            checked: gated_on,
            interaction: gate_interaction,
        }),
        &theme,
        &GroupBoxStyle::default(),
        inner,
    );

    Scene::Container(
        ContainerNode::new(vec![group])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Stretch)
                    .with_padding(Rect::new(24, 24, 24, 24)),
            ),
    )
}

/// Mark a painted widget root a §5.39 focus stop, leaving everything else about
/// it alone.
fn with_focus_stop(scene: Scene) -> Scene {
    match scene {
        Scene::Container(mut c) => {
            c.layout = c.layout.with_focusable(true);
            Scene::Container(c)
        }
        other => other,
    }
}

/// Read one tagged `CheckboxExternal`'s `(state, checked)` out of the state
/// scene.
fn read_checkbox(scene: &Scene, tag: &str) -> (CheckboxState, bool) {
    let Some(node) = scene.find_external_with_tag(tag) else {
        return (CheckboxState::Idle, false);
    };
    let Some(intro) = node.handle.introspect() else {
        return (CheckboxState::Idle, false);
    };
    let state = match intro.query("state") {
        Some(IntrospectValue::Text(name)) => CheckboxState::from_name_or_default(&name),
        _ => CheckboxState::Idle,
    };
    let checked = matches!(intro.query("checked"), Some(IntrospectValue::Bool(true)));
    (state, checked)
}

/// A checkbox external seeded to `checked`, so the first paint shows the state
/// the binding declares rather than a frame of the default.
///
/// The slot is `"checked"` — `ToggleExternal` names its bool `"value"` and a
/// checkbox names its `"checked"`, and writing the wrong one is a silent no-op
/// unless the `Result` is read. It cost `settings-panel` six unseeded
/// checkboxes, found while writing this (R1554), so this call `expect`s.
fn seeded(checked: bool) -> Box<dyn External> {
    let mut ext = CheckboxExternal::new();
    ext.intervene("checked", IntrospectValue::Bool(checked))
        .expect("CheckboxExternal accepts a bool on its `checked` slot");
    Box::new(ext)
}

struct GroupBoxApp;

impl WidgetCore for GroupBoxApp {
    type State = States;
    // Every state change arrives through `apply_key` or the router's per-tag
    // `"send"` dispatch, never the shell's enum-typed `keybinding` channel.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // The gate starts ON, so the app opens with a live group and the demo
        // can watch it die.
        seeded(true)
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        INNER_TAGS
            .iter()
            .map(|tag| ExtraExternal::new(*tag, seeded(false)))
            .collect()
    }

    fn tag() -> &'static str {
        GATE_TAG
    }

    fn read_state(scene: &Scene) -> States {
        let mut out: States = [(CheckboxState::Idle, false); N];
        out[0] = read_checkbox(scene, GATE_TAG);
        for (i, tag) in INNER_TAGS.iter().enumerate() {
            out[i + 1] = read_checkbox(scene, tag);
        }
        out
    }

    fn view(state: States, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // ARIA checkbox activation: Space only. A key aimed at a control inside
        // a disabled region never gets here — the region is not in the Tab
        // order, so nothing there can be `focused`.
        if key != "Space" {
            return false;
        }
        let Some(tag) = focused else { return false };
        let Some(node) = scene.find_external_with_tag_mut(tag) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        intro
            .invoke(
                "send",
                IntrospectValue::Text("KeyboardActivate".to_string()),
            )
            .is_ok()
    }

    fn event_name((): ()) -> &'static str {
        // Unreachable: no enum-typed event channel (see `type Event = ()`).
        ""
    }

    fn title() -> &'static str {
        "pinion hello-group-box (R1554 §5.39 inherited disabled)"
    }

    fn fmt_state_log(state: &States) -> String {
        let gate = if state[0].1 { "on" } else { "off" };
        format!(
            "gate {gate} / {} / {}",
            state[0].0.as_name(),
            INNER_TAGS
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{t}={}", state[i + 1].1))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

impl WidgetA11y for GroupBoxApp {
    /// `role=group` for the frame, `checkbox` for the gate and for each member.
    ///
    /// Nothing here says `disabled`. The assembler stamps it on every node the
    /// scene's regions cover (R1554), so a member cannot be announced as
    /// actionable while the pointer router and the Tab order are refusing it.
    fn access_node(state: &States, focused: Option<&str>) -> Vec<AccessNode> {
        let mut out = Vec::with_capacity(N + 1);
        out.push(
            AccessNode::new(GROUP_TAG, AriaRole::Group)
                // ARIA: the gate is the control that governs the region, which
                // is what `aria-controls` says. The toolkit's checkable group box
                // publishes no such relation.
                .with_name("Advanced"),
        );
        out.push(
            AccessNode::new(GATE_TAG, AriaRole::CheckBox)
                .with_name("Advanced")
                .with_state(AccessState::from_interaction(state[0].0, Some(state[0].1)))
                .with_value(AccessValue::Bool(state[0].1))
                .with_controls(REGION_TAG),
        );
        for (i, tag) in INNER_TAGS.iter().enumerate() {
            let (_, checked) = state[i + 1];
            out.push(
                AccessNode::new(*tag, AriaRole::CheckBox)
                    .with_name(INNER_LABELS[i])
                    .with_state(AccessState::from_interaction(state[i + 1].0, Some(checked)))
                    .with_value(AccessValue::Bool(checked)),
            );
        }
        let _ = focused;
        out
    }

    fn access_focus_target(_state: &States, focused: Option<&str>) -> Option<AccessFocus> {
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for GroupBoxApp {
    type Renderer = HelloGroupBoxRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GroupBoxApp>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::composite_tag::GroupBoxTag;
    use pinion_core::scene_disabled::{disabled_census, resolve_disabled};

    fn states(gate_on: bool) -> States {
        [
            (CheckboxState::Idle, gate_on),
            (CheckboxState::Idle, true),
            (CheckboxState::Idle, false),
        ]
    }

    /// The painted, cascade-resolved scene — what the shell hit-tests, what the
    /// focus enumeration reads, and what `scene/disabled` answers from.
    fn painted(gate_on: bool) -> Scene {
        let mut scene = pinion_core::Owner::new().run(|| view(&states(gate_on), &Frame::new()));
        resolve_disabled(&mut scene);
        scene
    }

    #[test]
    fn the_tag_consts_are_the_ssot_spellings() {
        // The gate external's tag must be the tag `view_group_box` PAINTS, or
        // clicks on the title would route nowhere. Pinned against the SSOT
        // rather than trusted, because the two are `&'static str` consts here
        // and a `format!` there.
        assert_eq!(GATE_TAG, GroupBoxTag::title(GROUP_TAG));
        assert_eq!(REGION_TAG, GroupBoxTag::content(GROUP_TAG));
        assert_eq!(GroupBoxApp::tag(), GATE_TAG);
    }

    #[test]
    fn a_live_group_has_three_tab_stops() {
        assert_eq!(
            painted(true).collect_focusable_tags(),
            vec![
                GATE_TAG.to_owned(),
                INNER_TAGS[0].to_owned(),
                INNER_TAGS[1].to_owned(),
            ],
            "the gate then its two members, in paint order",
        );
    }

    #[test]
    fn clearing_the_gate_leaves_the_gate_as_the_only_stop() {
        assert_eq!(
            painted(false).collect_focusable_tags(),
            vec![GATE_TAG.to_owned()],
            "Tab cannot park inside an inert region — and the binding says \
             nothing about Tab order anywhere",
        );
    }

    #[test]
    fn clearing_the_gate_publishes_the_cause_for_every_member() {
        let scene = painted(false);
        let census = disabled_census(&scene);
        let tags: Vec<&str> = census.iter().map(|d| d.tag.as_str()).collect();
        assert_eq!(
            tags,
            vec![REGION_TAG, INNER_TAGS[0], INNER_TAGS[1]],
            "the region and both members",
        );
        for member in census.iter().filter(|d| d.tag != REGION_TAG) {
            assert_eq!(
                member.declared_by.as_deref(),
                Some(REGION_TAG),
                "each names the node to act on — the column Qt lacks",
            );
        }
    }

    #[test]
    fn a_live_group_publishes_nothing() {
        assert!(disabled_census(&painted(true)).is_empty());
    }

    #[test]
    fn the_gate_is_never_inside_its_own_region() {
        // A gate that greyed itself could not be turned back on.
        let scene = painted(false);
        assert!(
            disabled_census(&scene)
                .iter()
                .all(|d| d.tag != GATE_TAG && d.tag != GROUP_TAG),
            "neither the group frame nor its title checkbox is gated",
        );
    }

    #[test]
    fn a_member_is_announced_disabled_without_this_binding_saying_so() {
        let scene = painted(false);
        let (nodes, _) = pinion_a11y::build_access_tree(
            &pinion_core::Owner::new(),
            Some(&scene),
            || GroupBoxApp::access_node(&states(false), None),
            || None,
        );
        let by = |tag: &str| {
            nodes
                .iter()
                .find(|n| n.tag == tag)
                .unwrap_or_else(|| panic!("no node {tag}"))
        };
        assert!(!by(GATE_TAG).state.disabled, "the gate stays actionable");
        for tag in INNER_TAGS {
            assert!(
                by(tag).state.disabled,
                "{tag} is announced disabled — `access_node` never sets it",
            );
        }
        assert_eq!(
            by(GATE_TAG).controls.as_deref(),
            Some(REGION_TAG),
            "and the gate publishes WHAT it governs (ARIA aria-controls)",
        );
    }

    #[test]
    fn a_press_aimed_at_a_gated_member_resolves_to_the_region() {
        // Needs real rects, so run the layout pass the shell runs.
        let mut scene = painted(false);
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, WIN_W, WIN_H);
        let rect = pinion_runtime::rect_for_tag(&scene, INNER_TAGS[0]).expect("painted");
        let hit = scene
            .hit_test(rect.x + rect.w / 2, rect.y + rect.h / 2)
            .expect("inside the window");
        assert!(
            hit.segments.iter().any(|s| s == REGION_TAG),
            "the press stops at the region: {:?}",
            hit.segments,
        );
        assert!(
            !hit.segments.iter().any(|s| s == INNER_TAGS[0]),
            "and never reaches the checkbox under the cursor",
        );
    }

    #[test]
    fn a_live_member_still_receives_its_press() {
        // The negative control for the test above — without it, a hit_test that
        // resolved to the region unconditionally would also pass.
        let mut scene = painted(true);
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, WIN_W, WIN_H);
        let rect = pinion_runtime::rect_for_tag(&scene, INNER_TAGS[0]).expect("painted");
        let hit = scene
            .hit_test(rect.x + rect.w / 2, rect.y + rect.h / 2)
            .expect("inside the window");
        assert!(
            hit.segments.iter().any(|s| s == INNER_TAGS[0]),
            "a live checkbox is hit normally: {:?}",
            hit.segments,
        );
    }

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<GroupBoxApp>(
            states(true),
            &Frame::new(),
        );
    }
}
