//! `hello-tooltip-rich` — R729 §5.38 §5.40 §5.50 Material 3 **rich
//! tooltip**: a hover / keyboard-focus popover that, unlike the plain
//! tooltip (`hello-tooltip`, R695/R723), carries a **title + supporting
//! paragraph on an elevated `surfaceContainer` surface**.
//!
//! ## substrate-0 composition (2nd-consumer round)
//!
//! The rich tooltip reuses the entire R695 substrate and adds **no new
//! coordinator**:
//!
//! - **visibility** — the same [`TooltipExternal`] visibility statechart
//!   (`(hovered || focused) && !dismissed`, WCAG 1.4.13 hoverable /
//!   dismissible / persistent). This binding is the 2nd standalone
//!   consumer of that widget after `hello-tooltip`.
//! - **positioning** — the same `anchor_position` flip / clamp
//!   positioner from `pinion_widget_paint::tooltip`.
//! - **elevation** — the shared `pinion_widget_paint::elevation` ramp
//!   (R711), the 5th consumer after dialog / menu / drawer / hello-
//!   elevation. This clears the R711/R723/R724 "elevated tooltip
//!   variant" carry: the plain tooltip is MD3 Level 0 (flat,
//!   inverseSurface); the rich tooltip is an elevated `surfaceContainer`.
//!
//! The only thing that diverges from the plain tooltip is the *paint*
//! (surfaceContainer + shadow + title/body layout), built inline here as
//! the 1st rich-tooltip consumer; a `pinion_widget_paint::tooltip`
//! rich-body helper is a future lift once a 2nd rich consumer appears
//! (`[[abstraction-needs-second-consumer]]`). Optional **action buttons**
//! (the other optional MD3 rich-tooltip slot) are deferred — composing
//! interactive children inside the hover-routed body needs the nested
//! hover-posture story, a separate axis.
//!
//! ## AI clients (§2 invariant #2)
//!
//! `query("visible" | "hovered" | "focused" | "dismissed")` reads the
//! posture; `scene/hover {at}` drives the hover trigger, `focus/set` the
//! focus trigger, and `scene/invoke {dismiss}` (or `Escape`) the WCAG
//! dismiss. The elevated body is queryable via `scene/snapshot` on the
//! `autosave#pop` node (fill / shadows / title + body text).

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widgets::tooltip::TooltipExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::elevation::elevation;
use pinion_widget_paint::tooltip::{anchor_position, TooltipPlacement, TooltipSide};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTooltipRichRenderer, HelloTooltipRichRendererError);

const WIN_W: u32 = 480;
const WIN_H: u32 = 320;
const THEME_TAG: &str = "app";

/// The trigger control tag — the tooltip body carries the same tag (plus
/// the `#pop` sub-tag) so the hover router treats trigger + body as one
/// contiguous hoverable region (WCAG 1.4.13).
const TRIGGER_TAG: &str = "autosave";
/// Trigger label (single source — paint + enriched a11y name).
const TRIGGER_LABEL: &str = "Auto-save";

/// Rich-tooltip content (single source — paint + the explicit a11y
/// description). A noun + descriptive paragraph, not a destructive verb:
/// a tooltip-only control must not imply a click action it does not do.
const TIP_TITLE: &str = "Auto-save";
const TIP_BODY: &str =
    "Saves your changes automatically every few seconds, so you never lose work.";

/// Fixed trigger rect (root-content coordinates) — high-left so the
/// tooltip opens below with room (no flip; flip / clamp is covered by the
/// plain hello-tooltip + the `anchor_position` unit tests).
const TRIGGER_RECT: Rect = Rect::new(40, 72, 170, 44);
/// Rich-tooltip box `(width, height)` — wide enough for the body to wrap
/// to ~3 lines, tall enough for the title + wrapped body + padding.
const TIP_SIZE: (u32, u32) = (256, 104);

/// Material-style elevation level for the transient rich-tooltip surface.
/// The `elevation` ramp disclaims bit-exact MD3 dp; this is a deliberate
/// low-but-visible cast (a rich tooltip floats above content like a
/// menu). Level 0 (flat) is the *plain* tooltip; the rich surface lifts.
const RICH_TOOLTIP_LEVEL: u8 = 2;

const CORNER_RADIUS: u32 = 12;
const PAD: u32 = 12;
const CONTENT_GAP: u32 = 4;
const TITLE_FONT_PX: u32 = 14;
const BODY_FONT_PX: u32 = 13;
/// M3 keyboard focus-ring width (mirrors the R694 button ring).
const FOCUS_RING_WIDTH: u32 = 3;
/// M3 hover state-layer weight (`onSurface` over the surface at 8 %).
const HOVER_STATE_LAYER: f32 = 0.08;

/// Tooltip posture read back from the state scene for the view + a11y.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
struct AnchorState {
    visible: bool,
    focused: bool,
    hovered: bool,
}

/// The `#pop` sub-tag the tooltip overlay carries (trigger tag + suffix)
/// so the hover router routes the body back to the trigger's external
/// while the overlay stays independently locatable.
fn pop_tag(anchor: &str) -> String {
    format!("{anchor}#pop")
}

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

/// Paint the trigger as an M3 chip — outlined surface, hover state-layer,
/// R694 keyboard focus ring (an accent border) when focused.
fn trigger_scene(posture: AnchorState, theme: &Theme) -> Scene {
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
            TRIGGER_LABEL,
            Rect::default(),
            TextStyle::new()
                .with_size_px(16)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(TRIGGER_TAG)
        .with_style(BoxStyle::filled(fill).with_corner_radius(8).with_border(border))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_absolute_position(TRIGGER_RECT.x, TRIGGER_RECT.y)
                .with_size(Size::px(TRIGGER_RECT.w, TRIGGER_RECT.h))
                .with_focusable(true),
        ),
    )
}

/// Compose the rich-tooltip overlay: an elevated `surfaceContainer`
/// surface carrying a title + supporting paragraph, positioned against
/// the trigger by the shared `anchor_position`. Tagged with the trigger's
/// `#pop` sub-tag for the hoverable contract. Built inline (1st rich
/// consumer); a shared rich-body helper waits for a 2nd consumer.
fn view_rich_tooltip(theme: &Theme) -> Scene {
    let placement = TooltipPlacement {
        anchor: TRIGGER_RECT,
        tip_size: TIP_SIZE,
        side: TooltipSide::Below,
    };
    let (left, top) = anchor_position(&placement, (WIN_W, WIN_H));
    let (tip_w, tip_h) = TIP_SIZE;

    let title = Scene::Text(TextNode::styled(
        TIP_TITLE,
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let body = Scene::Text(TextNode::styled(
        TIP_BODY,
        Rect::default(),
        TextStyle::new()
            .with_size_px(BODY_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![title, body])
            .with_tag(pop_tag(TRIGGER_TAG))
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainer))
                    .with_corner_radius(CORNER_RADIUS)
                    .with_shadows(elevation(RICH_TOOLTIP_LEVEL)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_justify(JustifyContent::Start)
                    .with_gap(CONTENT_GAP)
                    .with_absolute_position(left, top)
                    .with_size(Size::px(tip_w, tip_h))
                    .with_padding(Rect::new(PAD, PAD, PAD, PAD)),
            ),
    )
}

/// view-fn (§6.3): pure sync `AnchorState -> Scene`. The trigger paints
/// at its fixed rect; the rich tooltip is appended **last** (absolutely
/// positioned over the content) so it paints on top while visible.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: AnchorState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let header = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            "Hover or Tab to the control for its rich tooltip.  Esc dismisses.",
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

    let mut children = vec![header, trigger_scene(state, &theme)];
    if state.visible {
        children.push(view_rich_tooltip(&theme));
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

struct RichTooltipView;

impl WidgetCore for RichTooltipView {
    type State = AnchorState;
    // Driven by hover (router) + focus (FocusManager) + Escape (apply_key)
    // + the RPC invoke channel; no keybinding-channel events flow through
    // `event_name` (mirrors hello-tooltip).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(TooltipExternal::new())
    }

    fn tag() -> &'static str {
        TRIGGER_TAG
    }

    fn read_state(scene: &Scene) -> AnchorState {
        read_anchor(scene, TRIGGER_TAG)
    }

    fn view(state: AnchorState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-tooltip-rich (R729 §5.38 §5.40 §5.50)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// R695 §5.35 — `Escape` dismisses the shown tooltip without moving
    /// hover / focus (WCAG 1.4.13 dismissible), routed to the same
    /// `dismiss` invoke the RPC `scene/invoke` action uses (one funnel).
    fn apply_key(
        scene: &mut Scene,
        _focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if key != "Escape" {
            return false;
        }
        let Some(intro) = scene
            .find_external_with_tag_mut(TRIGGER_TAG)
            .and_then(|node| node.handle.introspect_mut())
        else {
            return false;
        };
        if matches!(intro.query("visible"), Some(IntrospectValue::Bool(true))) {
            let _ = intro.invoke("dismiss", IntrospectValue::Null);
            true
        } else {
            false
        }
    }

    fn fmt_state_log(state: &AnchorState) -> String {
        format!(
            "autosave(vis={} foc={} hov={})",
            state.visible, state.focused, state.hovered
        )
    }
}

impl WidgetA11y for RichTooltipView {
    /// R729 §5.40 — the trigger is an [`AriaRole::Button`]; while the
    /// tooltip is shown it gains an `aria-describedby` to an
    /// [`AriaRole::Tooltip`] node (the `#pop` overlay). The tooltip node's
    /// name is set **explicitly** to `"<title>. <body>"` (the rich body
    /// has two text nodes, so name-from-contents enrichment is ambiguous;
    /// an explicit name survives enrichment — the R728 lesson). The
    /// relation + node appear only while visible, so the described-by
    /// `NodeId` never dangles.
    fn access_node(state: &AnchorState, focused: Option<&str>) -> Vec<AccessNode> {
        let control = AccessNode::new(TRIGGER_TAG, AriaRole::Button).with_state(AccessState {
            focused: focused == Some(TRIGGER_TAG),
            hovered: state.hovered,
            ..AccessState::default()
        });
        // R759 — the describedby-gated region SSOT (shared with
        // hello-tooltip / hello-badge): link the trigger to the tooltip
        // region while shown, drop both when hidden (no dangling ref).
        pinion_a11y::describedby_region(
            control,
            pop_tag(TRIGGER_TAG),
            AriaRole::Tooltip,
            Some(format!("{TIP_TITLE}. {TIP_BODY}")),
            state.visible,
        )
    }
}

impl WidgetView for RichTooltipView {
    type Renderer = HelloTooltipRichRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<RichTooltipView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_tooltip_emits_only_the_trigger_button() {
        let nodes = RichTooltipView::access_node(&AnchorState::default(), None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert!(nodes[0].described_by.is_none(), "no dangling describedby when hidden");
    }

    #[test]
    fn visible_tooltip_adds_describedby_tooltip_node() {
        let state = AnchorState {
            visible: true,
            focused: true,
            hovered: true,
        };
        let nodes = RichTooltipView::access_node(&state, Some(TRIGGER_TAG));
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert!(nodes[0].state.focused);
        assert_eq!(nodes[0].described_by.as_deref(), Some("autosave#pop"));
        assert_eq!(nodes[1].role, AriaRole::Tooltip);
        assert_eq!(nodes[1].name.as_deref(), Some("Auto-save. Saves your changes automatically every few seconds, so you never lose work."));
    }

    #[test]
    fn rich_tooltip_paints_elevated_surface_container() {
        let theme = pinion_core::theme::Theme::light();
        let scene = pinion_core::Owner::new().run(|| view_rich_tooltip(&theme));
        let Scene::Container(c) = &scene else {
            panic!("rich tooltip is a container");
        };
        assert_eq!(c.tag.as_deref(), Some("autosave#pop"), "carries the #pop hoverable tag");
        assert_eq!(c.style.fill, theme.resolve(ColorRole::SurfaceContainer));
        assert_eq!(c.style.corner_radius, CORNER_RADIUS);
        assert!(!c.style.shadows.is_empty(), "elevated: casts a shadow");
        // Title + supporting paragraph.
        assert_eq!(c.children.len(), 2);
    }

    #[test]
    fn rich_tooltip_title_and_body_are_single_sourced() {
        let theme = pinion_core::theme::Theme::light();
        let scene = pinion_core::Owner::new().run(|| view_rich_tooltip(&theme));
        let Scene::Container(c) = &scene else {
            panic!("container");
        };
        let Scene::Text(title) = &c.children[0] else {
            panic!("title text");
        };
        let Scene::Text(body) = &c.children[1] else {
            panic!("body text");
        };
        assert_eq!(title.content, TIP_TITLE);
        assert_eq!(body.content, TIP_BODY);
    }

    #[test]
    fn escape_dismisses_a_shown_tooltip() {
        use pinion_core::scene::ExternalNode;
        let mut scene = Scene::External(
            ExternalNode::new(Box::new(TooltipExternal::new())).with_tag(TRIGGER_TAG),
        );
        // Show it via the hover channel first.
        if let Scene::External(node) = &mut scene {
            let intro = node.handle.introspect_mut().unwrap();
            intro
                .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
                .unwrap();
        }
        assert!(RichTooltipView::apply_key(
            &mut scene,
            None,
            "Escape",
            pinion_core::Modifiers::empty(),
        ));
        // A 2nd Escape with nothing shown is a no-op (returns false).
        assert!(!RichTooltipView::apply_key(
            &mut scene,
            None,
            "Escape",
            pinion_core::Modifiers::empty(),
        ));
    }
}
