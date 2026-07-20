//! `hello-stat-tiles` — R1385 a KPI **stat-tile row**: label + big value +
//! semantic delta + a trend **sparkline**.
//!
//! The forcing consumer for [`pinion_chart::Sparkline`]. Each tile is a card
//! surface holding a metric name, its current value, a period-over-period delta
//! (coloured by SIGN — a rise green, a fall red, the semantic colours the
//! dashboard brief keeps distinct from any brand accent), and a compact
//! sparkline of the metric's recent trend. This is the "Health Tiles" widget a
//! monitoring dashboard leads with, and the first consumer that composes several
//! sparklines in one scene — so each gets a DISTINCT tag prefix
//! (`spark_{i}`), the collision guard [`Sparkline`] documents.
//!
//! Display-only (PR-51 `primary_surface() -> None`, the `hello-chart-fill`
//! mould): a KPI tile has no gesture — it is read, not driven. Its introspection
//! IS the deliverable (§2 #7): every value, delta, colour, and sparkline mark is
//! queryable as scene data, which is what the RPC demo verifies.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, Sparkline};
use pinion_core::external::External;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::{ExtraExternal, PrimarySurface};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloStatTilesRenderer, HelloStatTilesRendererError);

const WIN_W: u32 = 720;
const WIN_H: u32 = 220;
const THEME_TAG: &str = "app";

/// KPI tile count.
const N: usize = 4;

const MARGIN: u32 = 20;
const GAP: u32 = 16;
// N = 4 tiles: 3 inter-tile gaps, 4 equal columns across the content width.
const TILE_W: u32 = (WIN_W - 2 * MARGIN - 3 * GAP) / 4;
const TILE_H: u32 = 150;
const TILE_Y: u32 = 46;
const TILE_PAD: u32 = 14;

/// A rising delta is drawn green, a falling one red — the SEMANTIC colours the
/// dashboard brief keeps deliberately distinct from any brand accent, and
/// theme-independent so they read on both light and dark.
const UP_COLOR: Color = Color::rgb(0x2E, 0xA0, 0x43);
const DOWN_COLOR: Color = Color::rgb(0xD9, 0x3A, 0x2C);

/// One KPI: a metric name, its current (pre-formatted) value, a
/// period-over-period delta in %, an 8-point recent trend, and the sparkline /
/// value accent colour.
struct Kpi {
    name: &'static str,
    value: &'static str,
    delta: f64,
    trend: [f64; 8],
    color: Color,
}

/// The four KPIs — all framed so "higher is better", so a positive delta always
/// reads as good (green) and the one negative delta (uptime dipping) reads as
/// bad (red). Deterministic data (ZERO-FLAKE).
const KPIS: [Kpi; N] = [
    Kpi {
        name: "Sessions",
        value: "1,284",
        delta: 12.4,
        trend: [800.0, 920.0, 880.0, 1050.0, 1180.0, 1120.0, 1240.0, 1284.0],
        color: Color::rgb(0x42, 0x85, 0xf4),
    },
    Kpi {
        name: "Throughput",
        value: "3.4k/s",
        delta: 5.1,
        trend: [2.6, 2.8, 3.0, 2.9, 3.1, 3.3, 3.2, 3.4],
        color: Color::rgb(0x34, 0xa8, 0x53),
    },
    Kpi {
        name: "Requests",
        value: "892k",
        delta: 2.0,
        trend: [820.0, 840.0, 835.0, 860.0, 870.0, 880.0, 885.0, 892.0],
        color: Color::rgb(0xf0, 0x9d, 0x00),
    },
    Kpi {
        name: "Uptime",
        value: "99.2%",
        delta: -0.3,
        trend: [99.6, 99.5, 99.6, 99.4, 99.5, 99.3, 99.4, 99.2],
        color: Color::rgb(0x0e, 0x9a, 0xa7),
    },
];

/// The per-tile card / sparkline tag prefix (`tile_0` .. `tile_3`).
fn tile_tag(i: usize) -> String {
    format!("tile_{i}")
}

/// The sparkline tag prefix for tile `i` — DISTINCT per tile so the several
/// `spark.*` trees never collide (the [`Sparkline`] row-collision rule).
fn spark_tag(i: usize) -> String {
    format!("spark_{i}")
}

/// The delta as a signed percentage string (`+12.4%` / `-0.3%`).
fn delta_text(delta: f64) -> String {
    format!("{delta:+.1}%")
}

/// A rise is green, a fall red — the sign drives the semantic colour.
fn delta_color(delta: f64) -> Color {
    if delta >= 0.0 { UP_COLOR } else { DOWN_COLOR }
}

/// Tile `i`'s window-absolute card rect.
fn tile_rect(i: usize) -> Rect {
    let col = u32::try_from(i).expect("tile index fits u32");
    let x = MARGIN + col * (TILE_W + GAP);
    Rect::new(x, TILE_Y, TILE_W, TILE_H)
}

/// A themed text label placed at an absolute window position.
fn text_at(content: &str, tag: &str, x: u32, y: u32, size: u32, fg: Color) -> Scene {
    Scene::Text(
        TextNode::styled(
            content,
            Rect::default(),
            TextStyle::new().with_size_px(size).with_fg(fg),
        )
        .with_tag(tag.to_string())
        .with_layout(LayoutStyle::new().with_absolute_position(x, y)),
    )
}

/// The sparkline style: no background (the card tone shows through), small
/// markers, and the theme's muted tone for the min / max reference dots.
fn spark_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        background: None,
        marker_radius: 3,
        ..ChartStyle::default()
    }
}

/// One KPI tile: a card surface + label + value + delta + trend sparkline, all
/// window-absolute inside the tile rect.
fn tile(i: usize, theme: &Theme) -> Vec<Scene> {
    let k = &KPIS[i];
    let r = tile_rect(i);
    let card = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow)).with_corner_radius(10),
        )
        .with_tag(tile_tag(i))
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(r.x, r.y)
                .with_size(Size::px(r.w, r.h)),
        ),
    );
    let label = text_at(
        k.name,
        &format!("{}.label", tile_tag(i)),
        r.x + TILE_PAD,
        r.y + 13,
        12,
        theme.resolve(ColorRole::OnSurfaceMuted),
    );
    let value = text_at(
        k.value,
        &format!("{}.value", tile_tag(i)),
        r.x + TILE_PAD,
        r.y + 30,
        26,
        theme.resolve(ColorRole::OnSurface),
    );
    let delta = text_at(
        &delta_text(k.delta),
        &format!("{}.delta", tile_tag(i)),
        r.x + TILE_PAD,
        r.y + 66,
        13,
        delta_color(k.delta),
    );
    let spark_rect = Rect::new(r.x + 12, r.y + 88, TILE_W - 24, TILE_H - 88 - 12);
    let spark = Sparkline::new(k.trend.to_vec())
        .with_color(k.color)
        .filled(true)
        .with_markers(true)
        .with_tag_prefix(spark_tag(i))
        .build(spark_rect, &spark_style(theme));
    vec![card, label, value, delta, spark]
}

/// view-fn (§6.3): pure sync `() -> Scene`. A row of KPI tiles over a themed
/// surface. No state — a stat tile is read, not driven.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let mut children = vec![text_at(
        "Key performance indicators — last 24h",
        "title",
        MARGIN,
        16,
        16,
        theme.resolve(ColorRole::OnSurface),
    )];
    for i in 0..N {
        children.extend(tile(i, &theme));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

struct StatTilesView;

impl WidgetCore for StatTilesView {
    type State = ();
    type Event = ();

    /// (PR-51) Display-only: no primary surface, no statechart, no gesture — a
    /// KPI tile is read, not driven.
    fn primary_surface() -> Option<PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("hello-stat-tiles has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("hello-stat-tiles has no primary surface — see primary_surface()")
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        Vec::new()
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-stat-tiles (R1385 KPI tiles + trend sparklines)"
    }

    fn fmt_state_log(_state: &Self::State) -> String {
        "display-only (KPI tiles are read, not driven)".to_string()
    }
}

impl WidgetA11y for StatTilesView {
    /// One [`AriaRole::Group`] per tile, named
    /// with the full KPI readout (`"Sessions: 1,284, +12.4%"`), bounds resolved
    /// from the card's `tile_{i}` tag — so a screen reader announces each KPI as
    /// a labelled region rather than a scatter of loose text.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        (0..N)
            .map(|i| {
                let k = &KPIS[i];
                AccessNode::new(tile_tag(i), AriaRole::Group).with_name(format!(
                    "{}: {}, {}",
                    k.name,
                    k.value,
                    delta_text(k.delta)
                ))
            })
            .collect()
    }
}

impl WidgetView for StatTilesView {
    type Renderer = HelloStatTilesRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<StatTilesView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn render() -> Scene {
        Owner::new().run(|| view((), &Frame::new()))
    }

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        if let Scene::Container(c) = scene {
            return c.children.iter().find_map(|ch| find(ch, tag));
        }
        None
    }

    fn text_of<'a>(scene: &'a Scene, tag: &str) -> Option<&'a str> {
        match find(scene, tag)? {
            Scene::Text(t) => Some(t.content.as_str()),
            _ => None,
        }
    }

    fn text_fg(scene: &Scene, tag: &str) -> Color {
        match find(scene, tag).unwrap_or_else(|| panic!("{tag} present")) {
            Scene::Text(t) => t.style.fg_color,
            other => panic!("{tag} is text, got {other:?}"),
        }
    }

    #[test]
    fn every_tile_has_a_card_label_value_delta_and_sparkline() {
        let scene = render();
        for i in 0..N {
            assert!(find(&scene, &tile_tag(i)).is_some(), "tile {i} card");
            assert!(
                find(&scene, &format!("{}.label", tile_tag(i))).is_some(),
                "tile {i} label"
            );
            assert!(
                find(&scene, &format!("{}.value", tile_tag(i))).is_some(),
                "tile {i} value"
            );
            assert!(
                find(&scene, &format!("{}.delta", tile_tag(i))).is_some(),
                "tile {i} delta"
            );
            assert!(
                find(&scene, &format!("{}.line", spark_tag(i))).is_some(),
                "tile {i} sparkline"
            );
        }
    }

    #[test]
    fn each_tile_shows_its_metric_name_and_value() {
        let scene = render();
        for (i, k) in KPIS.iter().enumerate() {
            assert_eq!(
                text_of(&scene, &format!("{}.label", tile_tag(i))),
                Some(k.name)
            );
            assert_eq!(
                text_of(&scene, &format!("{}.value", tile_tag(i))),
                Some(k.value)
            );
        }
    }

    #[test]
    fn a_rising_delta_is_green_and_a_falling_one_is_red() {
        let scene = render();
        // Sessions / Throughput / Requests rise (green); Uptime falls (red).
        assert_eq!(text_fg(&scene, "tile_0.delta"), UP_COLOR, "rise is green");
        assert_eq!(text_fg(&scene, "tile_3.delta"), DOWN_COLOR, "fall is red");
        assert_eq!(text_of(&scene, "tile_0.delta"), Some("+12.4%"));
        assert_eq!(text_of(&scene, "tile_3.delta"), Some("-0.3%"));
    }

    #[test]
    fn each_sparkline_has_a_distinct_prefix_no_collision() {
        let scene = render();
        // The bare `spark.*` tag must NOT appear — every sparkline is prefixed.
        assert!(
            find(&scene, "spark.line").is_none(),
            "no un-prefixed sparkline (row-collision guard)"
        );
        for i in 0..N {
            assert!(find(&scene, &format!("{}.end", spark_tag(i))).is_some());
        }
    }

    #[test]
    fn sparklines_are_filled_area_charts_with_markers() {
        let scene = render();
        for i in 0..N {
            assert!(
                find(&scene, &format!("{}.area", spark_tag(i))).is_some(),
                "tile {i} sparkline is filled"
            );
            assert!(
                find(&scene, &format!("{}.end", spark_tag(i))).is_some(),
                "tile {i} sparkline has an end cap"
            );
        }
    }

    #[test]
    fn a11y_exposes_one_named_group_per_tile() {
        let nodes = StatTilesView::access_node(&(), None);
        assert_eq!(nodes.len(), N, "one a11y node per tile");
        assert_eq!(nodes[0].role, AriaRole::Group);
        assert_eq!(
            nodes[0].name.as_deref(),
            Some("Sessions: 1,284, +12.4%"),
            "the group is named with the full KPI readout"
        );
    }
}
