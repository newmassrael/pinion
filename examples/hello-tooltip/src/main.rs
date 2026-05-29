// R695 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (TooltipExternal, WAI-ARIA, TooltipStyle, WCAG, …).
#![allow(clippy::doc_markdown)]

//! `hello-tooltip` — R695 §5.16 §5.35 §5.40 §5.50 first consumer of the
//! [`pinion_core::widgets::tooltip::TooltipExternal`] widget, the
//! [`pinion_widget_paint::tooltip`] chrome, and the `scene/hover` RPC
//! input primitive.
//!
//! ## Why this binding exists
//!
//! Phase B widget-catalog entry. A descriptive tooltip — the contextual
//! "what does this do?" popup every pro DCC / IDE / CAD tool attaches to
//! its toolbar glyphs, and a direct step toward the northern-star
//! "Unreal-class editor self-hosted in pinion" (every icon button ships
//! one). It is the catalog's first **descriptive-class** widget
//! (WAI-ARIA `tooltip`): no command, no selection, no toggle — passive
//! text the trigger references through `aria-describedby`.
//!
//! ## Trigger + dismiss (WCAG 2.2 SC 1.4.13)
//!
//! Two trigger buttons (`save` = the primary external, `delete` = an
//! extra external) each own a [`TooltipExternal`]. A tooltip shows while
//! its trigger is **hovered or keyboard-focused** and hides once both
//! clear (*persistent* — no timer):
//!
//! - **Hover** — the `scene/hover` RPC (and a real cursor) drive the
//!   router's `PointerEnter` / `PointerLeave` arc into the trigger's
//!   external.
//! - **Focus** — Tab moves the shell focus between the two triggers;
//!   [`External::on_focus_change`](pinion_core::external::External)
//!   mirrors it into the tooltip statechart (the R694 channel).
//! - **Hoverable** — [`view_tooltip`] paints the overlay with the
//!   trigger's tag plus a `#pop` sub-index, so the router routes a hover
//!   over the tooltip body back to the *same* external (the cursor can
//!   rest on the tooltip without it vanishing).
//! - **Dismissible** — `Escape` (and the RPC `scene/invoke` `dismiss`
//!   action — one funnel) hides the tooltip while hover / focus stays
//!   put; the latch clears on the next trigger-episode edge.
//!
//! ## Anchored positioning (flip + clamp)
//!
//! [`anchor_position`] places each tooltip flush against its trigger,
//! flipping above when below would overflow the window and clamping
//! horizontally so it never paints off-screen. `save` sits high (tooltip
//! opens below); `delete` sits low at the right edge so its tooltip
//! demonstrates both the vertical flip *and* the horizontal clamp.
//!
//! ## AI clients (§2 invariant #2)
//!
//! `query("visible" | "hovered" | "focused" | "dismissed")` reads each
//! trigger's tooltip posture; `scene/hover {at}` drives the hover
//! trigger, `focus/set` the focus trigger, and `scene/invoke` with the
//! `dismiss` action the WCAG dismiss. The overlay is queryable via
//! `scene/snapshot` / `scene/bbox` on the `<trigger>#pop` node (§2
//! invariant #7).

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::tooltip::TooltipExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::tooltip::{view_tooltip, TooltipPlacement, TooltipSide, TooltipStyle};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTooltipRenderer, HelloTooltipRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";

/// Primary trigger button tag (the `save` toolbar glyph).
const SAVE_TAG: &str = "save";
/// Extra trigger button tag (the `delete` glyph, placed low + right).
const DELETE_TAG: &str = "delete";

/// Trigger labels (single source — the paint label + the enriched a11y
/// name both derive from these).
const SAVE_LABEL: &str = "Save";
const DELETE_LABEL: &str = "Delete";

/// Tooltip descriptions (single source — passed to [`view_tooltip`];
/// the a11y tooltip-node name is enriched from the painted text, so
/// there is no parallel hardcoded a11y copy to drift).
const SAVE_TIP: &str = "Saves the current document.";
const DELETE_TIP: &str = "Permanently deletes the file.";

/// Fixed trigger geometry (root-content coordinates). Both triggers are
/// absolutely positioned so the anchored-positioning math is
/// deterministic (and the demo can assert exact tooltip rects). `save`
/// is high-left (tooltip opens below, no flip); `delete` is low-right
/// (tooltip flips above + clamps left).
const SAVE_RECT: Rect = Rect::new(40, 64, 170, 44);
const DELETE_RECT: Rect = Rect::new(372, 296, 120, 44);

/// Tooltip box sizes (width chosen to fit each label single-line; height
/// is the M3 plain-tooltip single line).
const SAVE_TIP_SIZE: (u32, u32) = (210, 28);
const DELETE_TIP_SIZE: (u32, u32) = (220, 28);

/// M3 keyboard focus-ring width (mirrors the R694 button ring).
const FOCUS_RING_WIDTH: u32 = 3;
/// M3 hover state-layer weight (`onSurface` over the surface at 8 %).
const HOVER_STATE_LAYER: f32 = 0.08;

/// Per-trigger tooltip posture read back from the state scene for the
/// view + a11y: is the tooltip shown, does the trigger hold keyboard
/// focus, is it hovered. `Copy` so the shell hands it into the paint
/// closure without lifetime gymnastics.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
struct AnchorState {
    visible: bool,
    focused: bool,
    hovered: bool,
}

/// `(save, delete)` postures.
type TooltipViewState = (AnchorState, AnchorState);

/// The `#pop` sub-tag the tooltip overlay carries (anchor tag + this
/// suffix) so the hover router routes the body back to the trigger's
/// external while the overlay stays independently locatable.
fn pop_tag(anchor: &str) -> String {
    format!("{anchor}#pop")
}

/// Read one trigger's [`AnchorState`] from the state scene by tag.
fn read_anchor(scene: &Scene, tag: &str) -> AnchorState {
    let Some(intro) = scene
        .find_external_with_tag(tag)
        .and_then(|node| node.handle.introspect())
    else {
        return AnchorState::default();
    };
    let bool_slot = |path: &str| matches!(intro.query(path), Some(IntrospectValue::Bool(true)));
    AnchorState {
        visible: bool_slot("visible"),
        focused: bool_slot("focused"),
        hovered: bool_slot("hovered"),
    }
}

/// Paint one trigger button as an M3 chip: an outlined surface with a
/// centred label, a hover state-layer, and the R694 keyboard focus ring
/// (an [`FOCUS_RING_WIDTH`]-px accent border) when focused.
fn trigger_scene(
    tag: &'static str,
    label: &str,
    rect: Rect,
    posture: AnchorState,
    theme: &pinion_core::theme::Theme,
) -> Scene {
    let base = theme.resolve(ColorRole::SurfaceContainerHighest);
    let fill = if posture.hovered {
        base.lerp(theme.resolve(ColorRole::OnSurface), HOVER_STATE_LAYER)
    } else {
        base
    };
    let border = if posture.focused {
        Border::new(theme.resolve(ColorRole::Accent), FOCUS_RING_WIDTH)
    } else {
        Border::new(theme.resolve(ColorRole::Outline), 1)
    };
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            label,
            Rect::default(),
            TextStyle::new()
                .with_size_px(16)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(tag)
        .with_style(BoxStyle::filled(fill).with_corner_radius(8).with_border(border))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h)),
        ),
    )
}

/// view-fn (§6.3): pure sync mapping `(save, delete postures) -> Scene`.
/// The two triggers paint at their fixed rects; each visible tooltip is
/// appended **last** (absolutely positioned over the content) so it
/// paints on top.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: TooltipViewState, _frame: &Frame) -> Scene {
    let (save, delete) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = TooltipStyle::m3_default();

    let header = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            "Hover or Tab to a button for its tooltip.  Esc dismisses.",
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(16, 16)
                .with_size(Size::px(WIN_W - 32, 24)),
        ),
    );

    let mut children = vec![
        header,
        trigger_scene(SAVE_TAG, SAVE_LABEL, SAVE_RECT, save, &theme),
        trigger_scene(DELETE_TAG, DELETE_LABEL, DELETE_RECT, delete, &theme),
    ];
    if save.visible {
        children.push(view_tooltip(
            pop_tag(SAVE_TAG),
            SAVE_TIP,
            &TooltipPlacement {
                anchor: SAVE_RECT,
                tip_size: SAVE_TIP_SIZE,
                side: TooltipSide::Below,
            },
            (WIN_W, WIN_H),
            &theme,
            &style,
        ));
    }
    if delete.visible {
        children.push(view_tooltip(
            pop_tag(DELETE_TAG),
            DELETE_TIP,
            &TooltipPlacement {
                anchor: DELETE_RECT,
                tip_size: DELETE_TIP_SIZE,
                side: TooltipSide::Below,
            },
            (WIN_W, WIN_H),
            &theme,
            &style,
        ));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(WIN_W, WIN_H))
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start),
            ),
    )
}

struct TooltipView;

impl WidgetCore for TooltipView {
    type State = TooltipViewState;
    // Tooltips are driven by hover (router) + focus (FocusManager) +
    // Escape (apply_key) + the RPC invoke channel; no keybinding-channel
    // events flow through `event_name` (mirrors hello-menu / hello-dialog).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(TooltipExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(DELETE_TAG, Box::new(TooltipExternal::new()))]
    }

    fn tag() -> &'static str {
        SAVE_TAG
    }

    fn read_state(scene: &Scene) -> TooltipViewState {
        (read_anchor(scene, SAVE_TAG), read_anchor(scene, DELETE_TAG))
    }

    fn view(state: TooltipViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-tooltip (R695 §5.35 §5.40 §5.50)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Both triggers are static Tab stops, so Tab moves focus between
    /// them and (via `on_focus_change`) shows the focused trigger's
    /// tooltip.
    fn focusable_tags() -> Vec<&'static str> {
        vec![SAVE_TAG, DELETE_TAG]
    }

    /// R695 §5.35 — `Escape` dismisses any shown tooltip without moving
    /// hover / focus (WCAG 1.4.13 dismissible). Routed to the same
    /// `dismiss` invoke the RPC `scene/invoke` action uses, so the human
    /// keyboard and an AI client converge on one funnel. Returns `true`
    /// when a tooltip was dismissed so the shell swallows the key.
    fn apply_key(
        scene: &mut Scene,
        _focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if key != "Escape" {
            return false;
        }
        let mut dismissed_any = false;
        for tag in [SAVE_TAG, DELETE_TAG] {
            let Some(intro) = scene
                .find_external_with_tag_mut(tag)
                .and_then(|node| node.handle.introspect_mut())
            else {
                continue;
            };
            let was_visible =
                matches!(intro.query("visible"), Some(IntrospectValue::Bool(true)));
            if was_visible {
                let _ = intro.invoke("dismiss", IntrospectValue::Null);
                dismissed_any = true;
            }
        }
        dismissed_any
    }

    fn fmt_state_log(state: &TooltipViewState) -> String {
        format!(
            "save(vis={} foc={}) delete(vis={} foc={})",
            state.0.visible, state.0.focused, state.1.visible, state.1.focused
        )
    }
}

impl WidgetA11y for TooltipView {
    /// R695 §5.40 — each trigger is an [`AriaRole::Button`] carrying its
    /// focus state; while its tooltip is shown it gains an
    /// `aria-describedby` relation to an [`AriaRole::Tooltip`] node (the
    /// overlay's `#pop` tag), so AT announces "Save, button, Saves the
    /// current document." The tooltip node's name is left `None` and
    /// enriched from the painted text by `enrich_names_from_scene` (the
    /// hello-dialog / hello-menu SSOT precedent — no parallel hardcoded
    /// a11y copy). The relation + tooltip node appear only while visible,
    /// so the described-by NodeId never dangles.
    fn access_node(state: &TooltipViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let (save, delete) = state;
        let mut nodes = Vec::new();
        for (tag, posture) in [(SAVE_TAG, save), (DELETE_TAG, delete)] {
            let mut trigger = AccessNode::new(tag, AriaRole::Button).with_state(AccessState {
                focused: focused == Some(tag),
                hovered: posture.hovered,
                ..AccessState::default()
            });
            if posture.visible {
                let desc = pop_tag(tag);
                trigger = trigger.with_described_by(desc.clone());
                nodes.push(trigger);
                nodes.push(AccessNode::new(desc, AriaRole::Tooltip));
            } else {
                nodes.push(trigger);
            }
        }
        nodes
    }
}

impl WidgetView for TooltipView {
    type Renderer = HelloTooltipRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TooltipView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_widget_paint::tooltip::anchor_position;

    fn hidden() -> AnchorState {
        AnchorState::default()
    }

    fn shown(focused: bool, hovered: bool) -> AnchorState {
        AnchorState {
            visible: true,
            focused,
            hovered,
        }
    }

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        if let Scene::Container(c) = scene {
            if c.tag.as_deref() == Some(tag) {
                return Some(c);
            }
            for child in &c.children {
                if let Some(found) = find_container(child, tag) {
                    return Some(found);
                }
            }
        }
        None
    }

    // ----- view: tooltip presence gated on visibility -----

    #[test]
    fn r695_hidden_tooltips_not_painted() {
        let scene = pinion_core::Owner::new()
            .run(|| view((hidden(), hidden()), &Frame::new()));
        assert!(find_container(&scene, &pop_tag(SAVE_TAG)).is_none());
        assert!(find_container(&scene, &pop_tag(DELETE_TAG)).is_none());
        // Triggers are always present.
        assert!(find_container(&scene, SAVE_TAG).is_some());
        assert!(find_container(&scene, DELETE_TAG).is_some());
    }

    #[test]
    fn r695_visible_tooltip_painted_at_anchored_position() {
        let scene = pinion_core::Owner::new()
            .run(|| view((shown(false, true), hidden()), &Frame::new()));
        let tip = find_container(&scene, &pop_tag(SAVE_TAG)).expect("save tooltip painted");
        // save sits high -> opens below, flush, no clamp.
        let expected = anchor_position(
            &TooltipPlacement {
                anchor: SAVE_RECT,
                tip_size: SAVE_TIP_SIZE,
                side: TooltipSide::Below,
            },
            (WIN_W, WIN_H),
        );
        assert_eq!(tip.layout.absolute_position, Some(expected));
        assert_eq!(expected, (40, 108), "below: 64 + 44 = 108, left = anchor.x");
    }

    #[test]
    fn r695_delete_tooltip_flips_above_and_clamps() {
        let scene = pinion_core::Owner::new()
            .run(|| view((hidden(), shown(true, false)), &Frame::new()));
        let tip = find_container(&scene, &pop_tag(DELETE_TAG)).expect("delete tooltip painted");
        let pos = tip.layout.absolute_position.expect("absolute");
        // delete is low-right: below would overflow (296+44+28=368 > 360)
        // -> flip above (296-28=268); right clamp (372+220=592 > 520) ->
        // 520-220=300.
        assert_eq!(pos, (300, 268));
    }

    // ----- view: focus ring -----

    #[test]
    fn r695_focused_trigger_paints_accent_ring() {
        let scene = pinion_core::Owner::new()
            .run(|| view((shown(true, false), hidden()), &Frame::new()));
        let save = find_container(&scene, SAVE_TAG).expect("save trigger");
        let border = save.style.border.expect("focused trigger has a ring");
        assert_eq!(border.width, FOCUS_RING_WIDTH);
        let delete = find_container(&scene, DELETE_TAG).expect("delete trigger");
        assert_eq!(
            delete.style.border.map(|b| b.width),
            Some(1),
            "unfocused trigger keeps the 1px outline, not the ring",
        );
    }

    // ----- a11y -----

    #[test]
    fn r695_hidden_emits_two_buttons_no_describedby() {
        let nodes = TooltipView::access_node(&(hidden(), hidden()), None);
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|n| n.role == AriaRole::Button));
        assert!(nodes.iter().all(|n| n.described_by.is_none()));
    }

    #[test]
    fn r695_visible_emits_tooltip_node_and_describedby() {
        let nodes = TooltipView::access_node(&(shown(false, true), hidden()), None);
        // save: button + tooltip ; delete: button.
        assert_eq!(nodes.len(), 3);
        let save = nodes.iter().find(|n| n.tag == SAVE_TAG).unwrap();
        assert_eq!(save.described_by.as_deref(), Some(pop_tag(SAVE_TAG).as_str()));
        let tip = nodes.iter().find(|n| n.role == AriaRole::Tooltip).unwrap();
        assert_eq!(tip.tag, pop_tag(SAVE_TAG));
    }

    #[test]
    fn r695_focused_trigger_marks_a11y_focus() {
        let nodes = TooltipView::access_node(&(hidden(), hidden()), Some(DELETE_TAG));
        let delete = nodes.iter().find(|n| n.tag == DELETE_TAG).unwrap();
        assert!(delete.state.focused);
        let save = nodes.iter().find(|n| n.tag == SAVE_TAG).unwrap();
        assert!(!save.state.focused);
    }

    #[test]
    fn r695_tooltip_name_enriched_from_paint_not_hardcoded() {
        // SSOT: access_node leaves the tooltip name None; enrich derives
        // it from the painted tooltip text (single source = SAVE_TIP).
        pinion_core::Owner::new().run(|| {
            let state = (shown(false, true), hidden());
            let scene = view(state, &Frame::new());
            let mut nodes = TooltipView::access_node(&state, None);
            let tip = nodes.iter().find(|n| n.role == AriaRole::Tooltip).unwrap();
            assert!(tip.name.is_none(), "access_node leaves the tooltip name to enrich");
            pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
            let tip = nodes.iter().find(|n| n.role == AriaRole::Tooltip).unwrap();
            assert_eq!(tip.name.as_deref(), Some(SAVE_TIP));
        });
    }

    // ----- apply_key: Escape dismiss -----

    #[test]
    fn r695_escape_dismisses_visible_tooltip() {
        use pinion_core::scene::ExternalNode;
        let mut scene = Scene::Container(ContainerNode::new(vec![
            Scene::External(ExternalNode::new(Box::new(TooltipExternal::new())).with_tag(SAVE_TAG)),
            Scene::External(
                ExternalNode::new(Box::new(TooltipExternal::new())).with_tag(DELETE_TAG),
            ),
        ]));
        // Show the save tooltip via hover.
        if let Some(intro) = scene
            .find_external_with_tag_mut(SAVE_TAG)
            .and_then(|n| n.handle.introspect_mut())
        {
            intro
                .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
                .unwrap();
        }
        let handled = TooltipView::apply_key(
            &mut scene,
            Some(SAVE_TAG),
            "Escape",
            pinion_core::Modifiers::empty(),
        );
        assert!(handled, "Escape dismissed a visible tooltip");
        let save = read_anchor(&scene, SAVE_TAG);
        assert!(!save.visible, "tooltip hidden after Escape while still hovered");
    }

    #[test]
    fn r695_escape_ignored_when_no_tooltip_shown() {
        use pinion_core::scene::ExternalNode;
        let mut scene = Scene::Container(ContainerNode::new(vec![Scene::External(
            ExternalNode::new(Box::new(TooltipExternal::new())).with_tag(SAVE_TAG),
        )]));
        assert!(!TooltipView::apply_key(
            &mut scene,
            None,
            "Escape",
            pinion_core::Modifiers::empty(),
        ));
    }

    #[test]
    fn r695_view_contains_trigger_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TooltipView>(
            (hidden(), hidden()),
            &Frame::default(),
        );
    }
}
