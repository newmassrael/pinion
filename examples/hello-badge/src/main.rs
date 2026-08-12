//! `hello-badge` — R759 §5.38 §5.40 §5.50 — first consumer of the
//! [`BadgeExternal`](pinion_core::widgets::badge) status holder: a
//! descriptive (non-interactive) **count / dot overlay** anchored to an
//! element, shown in its two Material 3 variants side by side —
//!
//!   * a **count** badge (a small pill carrying a number) over an "Inbox"
//!     anchor, demonstrating the `"{max}+"` overflow form;
//!   * a **dot** badge (a bare dot, "something new") over a "Sync"
//!     anchor.
//!
//! ## Why a badge is a plain holder (no statechart)
//!
//! A badge is *descriptive*: it owns no interaction state machine (no
//! pointer states, no keyboard model) and emits no §5.20 intents. R718
//! established that `operable` and `interactive-state` are orthogonal
//! axes — a badge is neither, so it is a hand-written [`External`]
//! value holder (the [`ProgressBarExternal`](pinion_core::widgets::progress_bar)
//! / [`TooltipExternal`](pinion_core::widgets::tooltip) mould), not an
//! SCXML-backed widget. Its observable axes — `count`, `max`, `dot`, plus
//! the derived read-only `label` (the capped display string) and
//! `visible` — are driven through the §5.15 introspect channel
//! (`intervene`), so an AI client setting `/external/count` and a host's
//! notification updater converge on one observable state.
//!
//! ## Composition (multi-external, the hello-card mould)
//!
//! N = 2 `BadgeExternal`s are composed through the R55.D.5
//! [`WidgetCore::create_extra_externals`] slot — the homogeneous cluster
//! path `hello-card` (3 cards) / `hello-accordion` (3 disclosures) use.
//! Booting one count badge **and** one dot badge means the boot frame
//! shows both variants at once, so the `PINION_SCREENSHOT` live-pixel
//! guard covers both without an interaction step.
//!
//! ## Positioning + paint are inline (1st badge consumer)
//!
//! The badge sits at the anchor's top-right corner via the existing
//! [`LayoutStyle::with_absolute_position`] field (the
//! [[absolute-position-via-layoutstyle]] idiom the progress indicator
//! and scrollbar thumb already use) — no new layout primitive. The pill
//! / dot paint is built **here**, in the single first consumer, following
//! the inline-first rule ([[abstraction-needs-second-consumer]]; the
//! R753 chip / R757 card precedent). It composes the existing Material 3
//! **error** colour tier (R590 `ColorRole::Error` / `OnError` — the
//! canonical notification-badge red). When a 2nd badge-paint consumer
//! appears the shared pill skin lifts to `pinion_widget_paint::badge`.
//!
//! ## a11y (reuses Status + aria-describedby — no new primitive)
//!
//! WAI-ARIA exposes a badge as a polite live region the anchor points
//! at: each anchor node carries `aria-describedby` → the badge's
//! [`AriaRole::Status`] node, whose name is the announcement ("3 unread",
//! "New activity"). Both reuse existing [`AccessNode`] fields
//! ([`with_described_by`](pinion_a11y::AccessNode::with_described_by) /
//! [`AriaRole::Status`], the `hello-tooltip` describedby + `hello-snackbar`
//! Status precedents). When a badge is hidden (`count == 0`, non-dot) the
//! describedby link and the Status node are both omitted — no dangling
//! reference (the `hello-tooltip` "no describedby when hidden" rule).

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y, describedby_region};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::badge::BadgeExternal;
use pinion_core::{Frame, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloBadgeRenderer, HelloBadgeRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 200;
/// [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key — the `"app"` convention shared across the
/// example gallery.
const THEME_TAG: &str = "app";

/// Badge count — one count badge + one dot badge.
const N: usize = 2;
const COUNT: usize = 0;
const DOT: usize = 1;

/// Per-badge `External` + paint-pill tags. Each tag lands on one badge
/// pill in the paint scene (so `scene/snapshot` / `rect_for_tag` resolve)
/// and on that badge's `External` in the state scene (so `read_state` /
/// RPC `/{tag}/external/*` route).
const BADGE_TAGS: [&str; N] = ["badge_count", "badge_dot"];
/// The anchor surfaces the badges decorate.
const ANCHOR_TAGS: [&str; N] = ["anchor_inbox", "anchor_sync"];
/// Anchor accessible names + the single glyph each icon surface shows.
const ANCHOR_NAMES: [&str; N] = ["Inbox", "Sync"];
const ANCHOR_GLYPHS: [&str; N] = ["I", "S"];
/// The count pill's text node tag — a deterministic mirror so the AI side
/// (and the demo) read the displayed label as text, no pixel round-trip.
const COUNT_LABEL_TAG: &str = "badge_count_label";

/// Icon-surface (anchor) box size.
const ANCHOR_SIZE: u32 = 64;
/// Badge pill height; the pill radius is half the height so the ends are
/// fully rounded (a stadium for 2+ digits, a circle for one).
const BADGE_H: u32 = 18;
const BADGE_RADIUS: u32 = 9;
/// Narrow pill width (1–2 glyphs) vs. wide pill width (3 glyphs, e.g.
/// `"99+"`). Picked by glyph count so a long label is not clipped.
const BADGE_W_NARROW: u32 = 18;
const BADGE_W_WIDE: u32 = 30;
/// Dot-variant size + radius.
const DOT_SIZE: u32 = 10;
const DOT_RADIUS: u32 = 5;

/// Boot count for the count badge — a clear, single-digit value so the
/// `"3"` pill is visible at startup for the `PINION_SCREENSHOT` guard.
const BOOT_COUNT: u32 = 3;

/// Cached projection of one badge's observable axes. `Copy` (the
/// `WidgetCore::State` bound) — the derived `label` / `visible` are not
/// stored but recomputed from the [`BadgeExternal`] SSOT in the view, so
/// the example never re-implements the capping / visibility logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadgeData {
    count: u32,
    max: u32,
    dot: bool,
}

pub type BadgeStates = [BadgeData; N];

/// Rebuild a [`BadgeExternal`] from a cached [`BadgeData`] so the view +
/// a11y read the capped `label` / `visible` through the widget's own
/// single source of truth (no duplicated overflow / visibility logic).
fn badge_of(d: BadgeData) -> BadgeExternal {
    let mut b = BadgeExternal::with_count(d.count);
    b.set_max(d.max);
    b.set_dot(d.dot);
    b
}

/// The badge's painted width: a small circle for a dot, a circle-ish
/// narrow pill for 1–2 glyphs, the wider stadium for `"99+"` (3 glyphs).
/// Shared by the paint (`badge_pill`) and the corner-position maths in
/// `view_anchor` so the pill is anchored against its true width.
fn badge_width(label: &str, dot: bool) -> u32 {
    if dot {
        DOT_SIZE
    } else if label.chars().count() <= 2 {
        BADGE_W_NARROW
    } else {
        BADGE_W_WIDE
    }
}

/// R759 §5.38 — the inline badge pill / dot paint (1st badge-paint
/// consumer), absolutely positioned at `(bx, by)` inside the anchor
/// stack. The error-tier red is the M3 notification-badge canonical
/// colour. A dot badge is a small filled circle; a count badge is a
/// stadium pill carrying the (capped) label in `OnError` text. The pill
/// node carries `tag` (= the badge `External` tag) so snapshot + rect
/// attach resolve.
fn badge_pill(tag: &'static str, label: &str, dot: bool, bx: u32, theme: &Theme) -> Scene {
    let bg = theme.resolve(ColorRole::Error);
    if dot {
        return Scene::Box(
            BoxNode::new(
                Rect::default(),
                BoxStyle::filled(bg).with_corner_radius(DOT_RADIUS),
            )
            .with_tag(tag)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(bx, 2)
                    .with_size(Size::px(DOT_SIZE, DOT_SIZE)),
            ),
        );
    }
    let text = Scene::Text(
        TextNode::styled(
            label.to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(11)
                .with_fg(theme.resolve(ColorRole::OnError)),
        )
        .with_tag(COUNT_LABEL_TAG)
        // R1650 §5.35 — the count is an ADDRESS for `scene/snapshot`, not a
        // control. Without this declaration it is the deepest tagged node over
        // the badge's whole rect, so the router resolved it, found no widget
        // behind the name and dropped every press on the badge — the R1497
        // shape, and `pointer_reach` reported it on its first sweep.
        .with_layout(LayoutStyle::new().with_pointer_transparent(true)),
    );
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(tag)
            .with_style(BoxStyle::filled(bg).with_corner_radius(BADGE_RADIUS))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_absolute_position(bx, 0)
                    .with_size(Size::px(badge_width(label, dot), BADGE_H)),
            ),
    )
}

/// One anchor: a fixed-size icon surface (a glyph centred on a
/// `SurfaceContainerHighest` tone) with the badge overlaid at the
/// top-right corner via `absolute_position`. Both children are absolutely
/// positioned out of flow inside the sized stack (the indeterminate
/// progress indicator's container shape), so the surface fill and the
/// badge never reflow each other. A hidden badge contributes no pill.
fn view_anchor(i: usize, data: BadgeData, theme: &Theme) -> Scene {
    let surface = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            ANCHOR_GLYPHS[i],
            Rect::default(),
            TextStyle::new()
                .with_size_px(24)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(ANCHOR_TAGS[i])
        .with_style(
            BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                .with_corner_radius(16),
        )
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_absolute_position(0, 0)
                .with_size(Size::px(ANCHOR_SIZE, ANCHOR_SIZE)),
        ),
    );

    let badge = badge_of(data);
    let mut children = vec![surface];
    if badge.visible() {
        let label = badge.label();
        // Top-right corner, flush against the surface's right edge.
        let bx = ANCHOR_SIZE.saturating_sub(badge_width(&label, data.dot));
        children.push(badge_pill(BADGE_TAGS[i], &label, data.dot, bx, theme));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_layout(LayoutStyle::new().with_size(Size::px(ANCHOR_SIZE, ANCHOR_SIZE))),
    )
}

/// view-fn (§6.3): pure sync mapping [`BadgeStates`] → [`Scene`]. A
/// centred row of the two badged anchors with their captions beneath.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &BadgeStates, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let columns: Vec<Scene> = (0..N)
        .map(|i| {
            let caption = Scene::Text(TextNode::styled(
                ANCHOR_NAMES[i],
                Rect::default(),
                TextStyle::new()
                    .with_size_px(13)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            ));
            Scene::Container(
                ContainerNode::new(vec![view_anchor(i, state[i], &theme), caption]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Column)
                        .with_align_items(AlignItems::Center)
                        .with_gap(10),
                ),
            )
        })
        .collect();
    let row = Scene::Container(
        ContainerNode::new(columns).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(48),
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

/// Read one badge's axes from its `External` introspect slots. Resolves
/// the tagged external **once** (the hello-progress multi-slot reader
/// shape — not one scene walk per slot), then reads `count` / `max` /
/// `dot`. The query values are always in `u32` range (the slots store a
/// `u32`), so an absent external falls back to the empty-badge defaults.
fn read_badge(scene: &Scene, tag: &str) -> BadgeData {
    let Some(intro) = scene
        .find_external_with_tag(tag)
        .and_then(|node| node.handle.introspect())
    else {
        return BadgeData {
            count: 0,
            max: BadgeExternal::DEFAULT_MAX,
            dot: false,
        };
    };
    let u32_slot = |path: &str, default: u32| {
        intro
            .query(path)
            .ok()
            .and_then(|v| v.as_i64())
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(default)
    };
    BadgeData {
        count: u32_slot("count", 0),
        max: u32_slot("max", BadgeExternal::DEFAULT_MAX),
        dot: matches!(intro.query("dot"), Ok(IntrospectValue::Bool(true))),
    }
}

pub struct BadgeView;

impl WidgetCore for BadgeView {
    type State = BadgeStates;
    // A badge emits no keyboard-channel events (mirrors hello-progress /
    // hello-tooltip): axis changes flow through the RPC / application
    // `intervene` channel, not `event_name`.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(BadgeExternal::with_count(BOOT_COUNT))
    }

    /// Badge 1 (the dot badge) as a sibling external; badge 0 (the count
    /// badge) is the primary [`Self::create_external`]. The substrate
    /// wraps the root as `Scene::Container([primary, ...extras])`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            BADGE_TAGS[DOT],
            Box::new(BadgeExternal::dot_badge()),
        )]
    }

    fn tag() -> &'static str {
        BADGE_TAGS[COUNT]
    }

    fn read_state(scene: &Scene) -> BadgeStates {
        [
            read_badge(scene, BADGE_TAGS[COUNT]),
            read_badge(scene, BADGE_TAGS[DOT]),
        ]
    }

    fn view(state: BadgeStates, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-badge (R759 §5.38 count / dot badge)"
    }

    fn fmt_state_log(state: &BadgeStates) -> String {
        let b0 = badge_of(state[COUNT]);
        let b1 = badge_of(state[DOT]);
        format!(
            "count='{}' visible={} | dot visible={}",
            b0.label(),
            b0.visible(),
            b1.visible(),
        )
    }
}

impl WidgetA11y for BadgeView {
    /// R759 §5.40 — per anchor: a generic anchor node carrying the
    /// anchor's accessible name, and (when the badge is visible) an
    /// `aria-describedby` link to the badge's [`AriaRole::Status`] live
    /// region whose name is the announcement. A hidden badge drops both
    /// the describedby link and the Status node — the
    /// [`describedby_region`] SSOT (shared with `hello-tooltip` /
    /// `hello-tooltip-rich`) owns the "no dangling reference when absent"
    /// gating.
    fn access_node(state: &BadgeStates, _focused: Option<&str>) -> Vec<AccessNode> {
        let mut nodes = Vec::with_capacity(N * 2);
        for i in 0..N {
            let badge = badge_of(state[i]);
            let anchor =
                AccessNode::new(ANCHOR_TAGS[i], AriaRole::Generic).with_name(ANCHOR_NAMES[i]);
            nodes.extend(describedby_region(
                anchor,
                BADGE_TAGS[i],
                AriaRole::Status,
                Some(announcement(state[i])),
                badge.visible(),
            ));
        }
        nodes
    }
}

/// The badge's accessible announcement (the `status` live-region name).
/// A dot badge announces presence ("New activity"); a count badge
/// announces the **raw** count (uncapped — more informative for AT than
/// the visually capped `"99+"`).
fn announcement(d: BadgeData) -> String {
    if d.dot {
        "New activity".to_string()
    } else {
        format!("{} unread", d.count)
    }
}

impl WidgetView for BadgeView {
    type Renderer = HelloBadgeRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<BadgeView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn count_data(count: u32) -> BadgeData {
        BadgeData {
            count,
            max: BadgeExternal::DEFAULT_MAX,
            dot: false,
        }
    }

    fn dot_data() -> BadgeData {
        BadgeData {
            count: 0,
            max: BadgeExternal::DEFAULT_MAX,
            dot: true,
        }
    }

    /// Build a fresh badge scene mirroring the shell's boot composition
    /// (`Scene::Container([primary, ...extras])`) so `read_state` walks
    /// the live topology.
    fn scene_fixture() -> Scene {
        let primary = Scene::External(
            ExternalNode::new(BadgeView::create_external()).with_tag(BADGE_TAGS[COUNT]),
        );
        let mut children = vec![primary];
        for extra in BadgeView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag.into_owned()),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }

    // ── read_state / composition ──────────────────────────────────────

    #[test]
    fn read_state_reflects_boot_count_and_dot() {
        let state = BadgeView::read_state(&scene_fixture());
        assert_eq!(state[COUNT].count, BOOT_COUNT);
        assert!(!state[COUNT].dot, "the count badge is not a dot");
        assert!(state[DOT].dot, "the dot badge boots as a dot");
    }

    #[test]
    fn read_state_reflects_count_intervene_with_overflow() {
        let mut scene = scene_fixture();
        if let Some(node) = scene.find_external_with_tag_mut(BADGE_TAGS[COUNT]) {
            node.handle
                .introspect_mut()
                .expect("opted in")
                .intervene("count", IntrospectValue::Int(150))
                .expect("count is writable");
        }
        let state = BadgeView::read_state(&scene);
        assert_eq!(state[COUNT].count, 150, "raw count round-trips uncapped");
        assert_eq!(
            badge_of(state[COUNT]).label(),
            "99+",
            "label caps at the overflow"
        );
    }

    #[test]
    fn read_state_defaults_without_external() {
        let data = read_badge(
            &Scene::Container(ContainerNode::new(vec![])),
            BADGE_TAGS[COUNT],
        );
        assert_eq!(data.count, 0);
        assert_eq!(data.max, BadgeExternal::DEFAULT_MAX);
        assert!(!data.dot);
    }

    // ── view ───────────────────────────────────────────────────────────

    #[test]
    fn view_carries_primary_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<BadgeView>(
            [count_data(BOOT_COUNT), dot_data()],
            &Frame::new(),
        );
    }

    #[test]
    fn view_paints_count_pill_and_dot_when_visible() {
        let scene = pinion_core::Owner::new()
            .run(|| view(&[count_data(BOOT_COUNT), dot_data()], &Frame::new()));
        assert!(scene.contains_tag(BADGE_TAGS[COUNT]), "count pill painted");
        assert!(scene.contains_tag(BADGE_TAGS[DOT]), "dot painted");
        assert!(scene_contains_text(&scene, "3"), "count label painted");
    }

    #[test]
    fn view_omits_count_pill_when_hidden() {
        // count 0, non-dot -> hidden: no pill in the paint scene (the
        // anchor surface still paints).
        let scene =
            pinion_core::Owner::new().run(|| view(&[count_data(0), dot_data()], &Frame::new()));
        assert!(
            !scene.contains_tag(BADGE_TAGS[COUNT]),
            "hidden count badge paints no pill"
        );
        assert!(
            scene.contains_tag(ANCHOR_TAGS[COUNT]),
            "the anchor surface still paints"
        );
    }

    fn scene_contains_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content == needle,
            Scene::Container(c) => c.children.iter().any(|c| scene_contains_text(c, needle)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn count_data(count: u32) -> BadgeData {
        BadgeData {
            count,
            max: BadgeExternal::DEFAULT_MAX,
            dot: false,
        }
    }

    fn dot_data() -> BadgeData {
        BadgeData {
            count: 0,
            max: BadgeExternal::DEFAULT_MAX,
            dot: true,
        }
    }

    #[test]
    fn anchor_describes_visible_badge_status_region() {
        let nodes = BadgeView::access_node(&[count_data(3), dot_data()], None);
        // anchor[0] -> describedby -> Status, anchor[1] -> describedby -> Status.
        let inbox = nodes
            .iter()
            .find(|n| n.tag == ANCHOR_TAGS[COUNT])
            .expect("inbox anchor");
        assert_eq!(inbox.role, AriaRole::Generic);
        assert_eq!(inbox.name.as_deref(), Some("Inbox"));
        assert_eq!(inbox.described_by.as_deref(), Some(BADGE_TAGS[COUNT]));
        let badge = nodes
            .iter()
            .find(|n| n.tag == BADGE_TAGS[COUNT])
            .expect("count badge node");
        assert_eq!(badge.role, AriaRole::Status);
        assert_eq!(badge.name.as_deref(), Some("3 unread"));
    }

    #[test]
    fn dot_badge_status_announces_presence() {
        let nodes = BadgeView::access_node(&[count_data(3), dot_data()], None);
        let badge = nodes
            .iter()
            .find(|n| n.tag == BADGE_TAGS[DOT])
            .expect("dot badge node");
        assert_eq!(badge.role, AriaRole::Status);
        assert_eq!(badge.name.as_deref(), Some("New activity"));
    }

    #[test]
    fn hidden_badge_drops_describedby_and_status_node() {
        // count 0, non-dot -> hidden.
        let nodes = BadgeView::access_node(&[count_data(0), dot_data()], None);
        let inbox = nodes
            .iter()
            .find(|n| n.tag == ANCHOR_TAGS[COUNT])
            .expect("inbox anchor");
        assert!(
            inbox.described_by.is_none(),
            "no dangling describedby when hidden"
        );
        assert!(
            !nodes.iter().any(|n| n.tag == BADGE_TAGS[COUNT]),
            "no Status node for a hidden badge",
        );
    }

    #[test]
    fn announcement_uses_uncapped_count() {
        // Even when the visible label caps at "99+", AT hears the real
        // magnitude.
        assert_eq!(announcement(count_data(150)), "150 unread");
    }

    #[test]
    fn badge_is_not_focusable() {
        // §5.39: collected from the paint scene — a badge marks no focus stop.
        let scene = pinion_core::Owner::new()
            .run(|| view(&[count_data(BOOT_COUNT), dot_data()], &Frame::new()));
        assert!(scene.collect_focusable_tags().is_empty());
    }
}
