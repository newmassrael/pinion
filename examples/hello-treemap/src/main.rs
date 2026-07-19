//! `hello-treemap` — R1382 consumer of the `pinion-chart` area-encoded
//! part-of-whole form: a [`pinion_chart::Treemap`] of a labelled asset-size
//! breakdown, laid out by the squarified algorithm, with a **scrub inspector**.
//!
//! ## What this demonstrates
//!
//! The treemap is built from retained primitives the paint adapter already
//! rasterizes: filled [`Scene::Box`] tiles (one per asset, area = share),
//! contrast-aware [`Scene::Text`] in-tile labels, and one interaction on top:
//!
//! * **Scrub inspect** (R1375's overlay, now for the treemap) — press-drag
//!   across the chart SCRUBS the tiles in descending-value order; the focused
//!   tile is framed by a ring and a tooltip shows its value + percent share.
//!   pinion forwards a continuous pointer position only under capture, so the
//!   gesture is a capture drag whose 1-D fraction the chart maps to a tile rank
//!   (a 2-D geometric hover over the tile grid would need a 2-D pointer external
//!   — deferred).
//!
//! ## Why a Slider
//!
//! The §5.38 [`SliderExternal`] (a captured 1-D fraction) is reused as the scrub
//! position, exactly as `hello-donut` reuses it — RPC-drivable
//! (`scene/intervene`) and introspectable, no new external invented.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the treemap + overlay as tagged data —
//! `chart.tile.{i}` / `chart.tile.{i}.label`, `chart.inspect.highlight` /
//! `.tooltip` / `.header` / `.value`. Driving the scrub over RPC re-rings a
//! different tile (its ring rect + the header both change), observed
//! structurally without OCR (§2 #1 / #7). See
//! `tools/demos/r1382_treemap_scrub.py`.

use pinion_a11y::described::describedby_region;
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, Tile, Treemap};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::widgets::slider::{SliderEvent, SliderExternal, SliderState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::slider::{read_slider_state, slider_apply_key};

// pinion-forge codegen output: `pub struct HelloTreemapRenderer` + …
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTreemapRenderer, HelloTreemapRendererError);

const WIN_W: u32 = 620;
const WIN_H: u32 = 460;

const THEME_TAG: &str = "app";
const SCRUB_TAG: &str = "treemap_scrub";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

/// Window-absolute treemap region. The chart is pinned (`Treemap::build`) — the
/// squarified tile geometry is what this demo exercises, not the layout-native
/// seam (the profiler already proves `build_fill`), so a const rect keeps it
/// simple. It is also the scrub capture basis: the `treemap_scrub` box covers
/// exactly this rect, so the slider value `0.0..=1.0` is the cursor fraction
/// across it.
const CHART_RECT: Rect = Rect::new(10, 40, WIN_W - 20, WIN_H - 74);

/// An illustrative project asset-size breakdown (MB) — the canonical
/// many-weighted-leaves-in-a-rectangle a treemap shows (a real asset browser /
/// disk-usage panel). Fixed sample data (like `hello-donut`'s `composition`):
/// the demo exists to prove the tile layout + the scrub inspector, not to
/// measure a real project. Already DESCENDING, so a tile's draw rank equals its
/// list position; the values sum to 680 (Textures = 35%).
fn assets() -> Vec<Tile> {
    vec![
        Tile::new("Textures", 240.0),
        Tile::new("Meshes", 180.0),
        Tile::new("Audio", 96.0),
        Tile::new("Animations", 62.0),
        Tile::new("Shaders", 44.0),
        Tile::new("Scripts", 30.0),
        Tile::new("Materials", 20.0),
        Tile::new("Fonts", 8.0),
    ]
}

/// Reused as the scrub-position holder, seeded to the centre so the boot frame
/// shows an inspected tile.
fn scrub_external() -> SliderExternal {
    let mut slider = SliderExternal::new();
    slider.set_value(0.5);
    slider
}

/// Resolve the theme into a [`ChartStyle`].
fn chart_style(theme: &pinion_core::Theme) -> ChartStyle {
    ChartStyle {
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        crosshair: theme.resolve(ColorRole::OnSurface),
        tooltip_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        tooltip_fg: theme.resolve(ColorRole::OnSurface),
        ..ChartStyle::default()
    }
}

/// view-fn (§6.3): pure sync `(SliderState, f32) -> Scene`. `scrub` is the
/// inspect fraction across [`CHART_RECT`] that scrubs the tiles.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: SliderState, scrub: f32, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);

    let title = Scene::Text(
        TextNode::styled(
            "Project assets (illustrative) — drag across the treemap to inspect a tile",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, 12)),
    );

    let treemap = Treemap::new(assets())
        .inspect(Some(scrub))
        .build(CHART_RECT, &chart_style(&theme));

    // Transparent capture surface over the treemap — the `treemap_scrub` primary
    // tag. On top so a press anywhere on the chart drives the scrub; transparent
    // so the tiles show through, pointer-opaque so it captures.
    let scrub_surface = Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
            .with_tag(SCRUB_TAG)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(CHART_RECT.x, CHART_RECT.y)
                    .with_size(Size::px(CHART_RECT.w, CHART_RECT.h)),
            ),
    );

    let status = Scene::Text(
        TextNode::styled(
            format!("{} | scrub {scrub:.2}", state.as_name()),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, WIN_H - 22)),
    );

    Scene::Container(
        ContainerNode::new(vec![treemap, scrub_surface, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// `WidgetView` binding. The Slider is the scrub position (primary); `#[widget]`
/// derives WidgetCore + WidgetView, and `a11y_manual` provides the hand-written
/// [`WidgetA11y`] below (the scrub Slider describedby the inspect region).
#[widget(
    tag = "treemap_scrub",
    state = (SliderState, f32),
    event = SliderEvent,
    title = "pinion hello-treemap (R1382 squarified treemap + scrub inspect)",
    renderer = HelloTreemapRenderer,
    initial_size = (WIN_W, WIN_H),
    external = scrub_external,
    apply_key,
    keybinding,
    event_name_derive,
    a11y_manual,
)]
struct TreemapView;

impl TreemapView {
    /// Reads the scrub fraction from the primary Slider external.
    fn read_state(scene: &Scene) -> (SliderState, f32) {
        read_slider_state(scene, SCRUB_TAG).unwrap_or((SliderState::Idle, 0.5))
    }

    fn view(state: (SliderState, f32), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    /// ARIA slider keyboard scrub, mirrored through the RPC `scene/intervene`
    /// value channel (the lifted `slider_apply_key`).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        slider_apply_key(scene, focused, Self::tag(), |current| match key {
            "ArrowLeft" | "ArrowDown" => Some((current - 0.1).clamp(0.0, 1.0)),
            "ArrowRight" | "ArrowUp" => Some((current + 0.1).clamp(0.0, 1.0)),
            "Home" => Some(0.0),
            "End" => Some(1.0),
            _ => None,
        })
    }

    fn keybinding(key: &str) -> Option<SliderEvent> {
        match key {
            "d" => Some(SliderEvent::Disable),
            "e" => Some(SliderEvent::Enable),
            _ => None,
        }
    }
}

/// The scrub surface carries `AriaRole::Slider` with the scrub fraction as its
/// `AccessValue::Float`, and is `describedby` the treemap's inspect region so
/// the tile + percent a sighted user reads in the tooltip reaches a screen
/// reader too (the R1355 parity, now for the treemap).
impl WidgetA11y for TreemapView {
    fn access_node(state: &(SliderState, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, scrub) = (state.0, state.1);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            ..AccessState::from_interaction(interaction, None)
        };
        // The readout must name the SAME tile the painted tooltip does. A treemap
        // scrub is rect-invariant — `resolve_focus` is `fraction * n` over the
        // tile COUNT, independent of the plot geometry — so this `CHART_RECT`
        // call resolves the identical tile the themed `build(CHART_RECT)` overlay
        // shows. (If a future 2-D geometric hover made focus geometry-dependent,
        // both call sites would then have to share one frame — the R1355 parity.)
        let readout = Treemap::new(assets())
            .inspect(Some(scrub))
            .inspect_readout(CHART_RECT);
        let control = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Slider)
            .with_value(AccessValue::Float {
                value: scrub,
                min: 0.0,
                max: 1.0,
            })
            .with_state(access_state);
        describedby_region(
            control,
            "chart.inspect.tooltip",
            AriaRole::Tooltip,
            readout,
            true,
        )
    }
}

fn main() {
    pinion_shell::run::<TreemapView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(scene);
                }
                c.children.iter().find_map(|ch| find(ch, tag))
            }
            other => (other.tag() == Some(tag)).then_some(scene),
        }
    }

    fn rendered(scrub: f32) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(SliderState::Idle, scrub, &Frame::new()))
    }

    #[test]
    fn treemap_and_scrub_surface_present() {
        let scene = rendered(0.5);
        assert!(find(&scene, "chart").is_some(), "the treemap root");
        assert!(
            find(&scene, SCRUB_TAG).is_some(),
            "the scrub capture surface"
        );
        for i in 0..assets().len() {
            assert!(
                find(&scene, &format!("chart.tile.{i}")).is_some(),
                "tile {i}"
            );
        }
    }

    #[test]
    fn inspect_overlay_tracks_the_scrub_value() {
        // A left-edge scrub and a right-edge scrub ring DIFFERENT tiles. Unlike
        // the donut (all sectors share one bbox), each treemap tile has its own
        // rect, so the ring's RECT is what moves.
        let left = rendered(0.0);
        let right = rendered(1.0);
        let ring_rect = |s: &Scene| {
            let Scene::Box(b) = find(s, "chart.inspect.highlight").expect("ring") else {
                panic!("ring is a box")
            };
            (b.rect.x, b.rect.y, b.rect.w, b.rect.h)
        };
        assert!(
            find(&left, "chart.inspect.tooltip").is_some(),
            "a tooltip at 0.0"
        );
        assert_ne!(
            ring_rect(&left),
            ring_rect(&right),
            "the ring frames a different tile at each end of the scrub"
        );
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TreemapView>(
            (SliderState::Idle, 0.5),
            &Frame::new(),
        );
    }

    #[test]
    fn r1360_2_view_paints_an_opaque_root() {
        pinion_core::test_fixtures::assert_widget_view_paints_opaque_root::<TreemapView>(
            (SliderState::Idle, 0.5),
            &Frame::new(),
        );
    }

    #[test]
    fn scrub_reports_slider_role_and_is_describedby_the_readout() {
        let nodes = <TreemapView as WidgetA11y>::access_node(&(SliderState::Idle, 0.5), None);
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert_eq!(nodes[0].tag, SCRUB_TAG);
        assert_eq!(
            nodes[0].described_by.as_deref(),
            Some("chart.inspect.tooltip"),
            "scrub is describedby the inspect region"
        );
        let region = nodes
            .iter()
            .find(|n| n.tag == "chart.inspect.tooltip")
            .expect("the described region is in the tree");
        let name = region.name.as_deref().expect("region carries the readout");
        assert!(
            name.contains('%'),
            "the readout names the tile's percent share: {name:?}"
        );
    }
}
