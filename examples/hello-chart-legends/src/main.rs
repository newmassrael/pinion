//! `hello-chart-legends` — R1722 §5.38 §5.39 §5.40 §2 #2 §2 #7 — a board that
//! **asks each chart what its legend can do** instead of knowing.
//!
//! # The screen
//!
//! Four chart kinds, side by side, each in a card whose caption is *derived
//! from the chart's own answer* rather than written per card:
//!
//! | card | chart | what it answers |
//! |---|---|---|
//! | Throughput | line, three series | three parts, any may be hidden |
//! | Traffic share | donut, three slices | three parts, any may be hidden — and the ring **re-normalises** |
//! | Profile | polar radar, two series | two parts, any may be hidden |
//! | Frame sizes | bar over categories | **no parts to name** |
//!
//! The fourth card is the point as much as the first three. Before R1722 a board
//! could not ask: two chart kinds in the whole crate offered the gesture, five
//! did not, and nothing anywhere said which was which — so a screen either knew
//! by having read the crate, or offered an affordance that did nothing. Here the
//! caption, the presence of a focus stop, and what a press does all come from
//! [`ChartLegend::legend`], and this file chooses none of them.
//!
//! # What it is a consumer of
//!
//! * [`pinion_chart::ChartLegend`] — the declaration, and the first consumer to
//!   hold **more than one chart kind at once**, which is the position that makes
//!   the question necessary. The three older legend demos each hold one chart of
//!   one kind and could get away with knowing.
//! * [`pinion_chart::LegendInteraction::Toggle`] on the **polar and donut
//!   charts**, which could not offer the gesture at all until this round.
//! * [`pinion_core::widgets::toggle_group`] — eight independent
//!   `ToggleExternal`s across three charts, each its own Tab stop, sharing the
//!   keyboard model / introspect reader / boot seed verbatim.
//! * [`ChartLegend::legend_access_nodes`] — three WAI-ARIA `group`s of
//!   `button[aria-pressed]`, one per chart with a toggle legend, derived rather
//!   than hand-built here. The card that names no parts contributes no nodes,
//!   which this file also does not decide.
//!
//! # Two hiding rules, and why they differ
//!
//! Hiding a line leaves the other lines where they were: the axes are measured
//! against every series, so a toggle never moves the grid. Hiding a slice
//! **re-normalises the ring**, because a part-of-whole picture whose parts no
//! longer sum to the whole is a lie. The difference is stated on
//! [`pinion_chart::Slice::visible`] and is visible on this screen by pressing
//! one entry in each of the first two cards.

use pinion_a11y::{AccessNode, WidgetA11y};
use pinion_chart::{
    Bar, BarChart, Categories, ChartLegend, ChartStyle, DataPoint, DonutChart, Legend,
    LegendInteraction, LegendPostures, LineChart, PolarChart, Series, Slice,
};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloChartLegendsRenderer, HelloChartLegendsRendererError);

const WIN_W: u32 = 920;
const WIN_H: u32 = 648;
const THEME_TAG: &str = "app";

const TITLE_FONT_PX: u32 = 15;
const CAPTION_FONT_PX: u32 = 12;

/// How many parts each chart with a toggle legend names. The bar chart is
/// absent from this list because it names none — which is its answer, not an
/// omission.
const THROUGHPUT_N: usize = 3;
const SHARE_N: usize = 3;
const PROFILE_N: usize = 2;

/// Every toggle on the board, in card order.
const N: usize = THROUGHPUT_N + SHARE_N + PROFILE_N;

/// Where each chart's run of toggles starts inside the flat state.
const THROUGHPUT_AT: usize = 0;
const SHARE_AT: usize = THROUGHPUT_AT + THROUGHPUT_N;
const PROFILE_AT: usize = SHARE_AT + SHARE_N;

/// The tag prefix each chart paints under, which is also what its legend entry
/// tags extend (R1722 derives `{prefix}.legend.{i}`).
const THROUGHPUT_TAG: &str = "throughput";
const SHARE_TAG: &str = "share";
const PROFILE_TAG: &str = "profile";
const SIZES_TAG: &str = "sizes";

/// Per-entry dispatch + Tab-stop tags — the tags the three toggle legends emit,
/// which the `ToggleExternal`s bind to, so a press on an entry routes straight
/// to its toggle.
///
/// Spelled out rather than built at runtime because each is a `&'static str`
/// §5.39 Tab stop, and asserted against the charts' own derivation by
/// `every_tag_this_file_binds_is_one_a_chart_derives` — so a prefix changed on
/// one side and not the other fails a test rather than silently unbinding a
/// legend entry.
const LEGEND_TAGS: [&str; N] = [
    "throughput.legend.0",
    "throughput.legend.1",
    "throughput.legend.2",
    "share.legend.0",
    "share.legend.1",
    "share.legend.2",
    "profile.legend.0",
    "profile.legend.1",
];

/// Every part starts shown.
const BOOT_ON: [bool; N] = [true; N];

/// Card geometry: four cards in two rows, each a chart above its caption.
const CARD_W: u32 = 420;
const CHART_H: u32 = 240;
const CARD_GAP: u32 = 20;
const ROW_0_Y: u32 = 56;
const ROW_1_Y: u32 = ROW_0_Y + CHART_H + 46;

const THROUGHPUT_RECT: Rect = Rect::new(CARD_GAP, ROW_0_Y, CARD_W, CHART_H);
const SHARE_RECT: Rect = Rect::new(CARD_GAP * 2 + CARD_W, ROW_0_Y, CARD_W, CHART_H);
const PROFILE_RECT: Rect = Rect::new(CARD_GAP, ROW_1_Y, CARD_W, CHART_H);
const SIZES_RECT: Rect = Rect::new(CARD_GAP * 2 + CARD_W, ROW_1_Y, CARD_W, CHART_H);

/// Series colours, so a swatch is comparable across cards.
const SERIES_COLORS: [Color; 3] = [
    Color::rgb(0x4d, 0xa6, 0xff),
    Color::rgb(0xff, 0xa8, 0x4d),
    Color::rgb(0x7d, 0xd8, 0x7d),
];

const THROUGHPUT_LABELS: [&str; THROUGHPUT_N] = ["ingress", "egress", "errors"];
const SHARE_LABELS: [&str; SHARE_N] = ["near", "middle", "far"];
const PROFILE_LABELS: [&str; PROFILE_N] = ["baseline", "measured"];

/// The radar's spokes, which are its angular axis rather than its legend — the
/// distinction the bar card makes in the other direction.
const PROFILE_SPOKES: [&str; 6] = ["setup", "encode", "send", "receive", "decode", "apply"];

/// The line chart's samples — one row per series.
fn throughput(visible: [bool; THROUGHPUT_N]) -> LineChart {
    let ys: [[f64; 8]; THROUGHPUT_N] = [
        [220.0, 280.0, 240.0, 340.0, 300.0, 420.0, 380.0, 460.0],
        [180.0, 150.0, 210.0, 190.0, 260.0, 230.0, 300.0, 280.0],
        [12.0, 30.0, 18.0, 44.0, 26.0, 60.0, 38.0, 52.0],
    ];
    let series = (0..THROUGHPUT_N)
        .map(|i| {
            Series::new(
                THROUGHPUT_LABELS[i],
                ys[i]
                    .iter()
                    .enumerate()
                    .map(|(k, &y)| {
                        #[allow(
                            clippy::cast_precision_loss,
                            reason = "eight sample indices convert exactly"
                        )]
                        DataPoint::new(k as f64, y)
                    })
                    .collect(),
            )
            .with_color(SERIES_COLORS[i])
            .with_visible(visible[i])
        })
        .collect();
    LineChart::new(series)
        .with_tag_prefix(THROUGHPUT_TAG)
        .with_legend(LegendInteraction::Toggle)
}

/// The donut chart's slices. Hiding one re-normalises the ring, which is the
/// difference from the line chart's rule and the reason both are on this board.
fn share(visible: [bool; SHARE_N]) -> DonutChart {
    let values = [52.0, 31.0, 17.0];
    let slices = (0..SHARE_N)
        .map(|i| {
            Slice::new(SHARE_LABELS[i], values[i])
                .with_color(SERIES_COLORS[i])
                .shown(visible[i])
        })
        .collect();
    DonutChart::new(slices)
        .with_tag_prefix(SHARE_TAG)
        .with_legend(LegendInteraction::Toggle)
}

/// The radar, whose legend is the natural place to compare one profile against
/// another by turning the rest off — a gesture this chart kind could not offer
/// before R1722.
fn profile(visible: [bool; PROFILE_N]) -> PolarChart {
    let rows: [[f64; 6]; PROFILE_N] = [
        [70.0, 62.0, 55.0, 68.0, 74.0, 60.0],
        [48.0, 81.0, 66.0, 40.0, 59.0, 77.0],
    ];
    let series = (0..PROFILE_N)
        .map(|i| {
            Series::new(
                PROFILE_LABELS[i],
                rows[i]
                    .iter()
                    .enumerate()
                    .map(|(k, &r)| {
                        #[allow(
                            clippy::cast_precision_loss,
                            reason = "six category indices convert exactly"
                        )]
                        DataPoint::new(k as f64, r)
                    })
                    .collect(),
            )
            .with_color(SERIES_COLORS[i])
            .with_visible(visible[i])
        })
        .collect::<Vec<_>>();
    PolarChart::radar(series, Categories::new(PROFILE_SPOKES))
        .with_tag_prefix(PROFILE_TAG)
        .with_legend(LegendInteraction::Toggle)
}

/// One set of labelled bars over a category axis. It has no second named thing
/// for a legend to distinguish, and says so.
fn sizes() -> BarChart {
    BarChart::new(vec![
        Bar::new("64", 18.0),
        Bar::new("128", 31.0),
        Bar::new("512", 46.0),
        Bar::new("1500", 27.0),
    ])
    .with_tag_prefix(SIZES_TAG)
}

/// What the board says under a card — **asked of the chart**, which is the whole
/// screen.
///
/// Three sentences, and nothing here knows which chart it is describing. A card
/// whose chart names no parts says so; a card whose legend is paint says that
/// too, and would offer no press.
fn caption(legend: &Legend) -> String {
    match (legend.len(), legend.interaction()) {
        (0, _) => "no parts to name".to_string(),
        (n, LegendInteraction::Toggle) => format!("{n} parts — press one to hide it"),
        (n, LegendInteraction::Paint) => format!("{n} parts — the legend is paint"),
    }
}

/// The themed chart style. `legend: true` — whether a row is *drawn* is the
/// style's question; what may be *done* to it is the chart's.
///
/// `x_ticks` / `y_ticks` are turned down from the default because these cards
/// are 420 px wide: a board that asks for the default tick count in a card this
/// size paints its axis labels on top of each other, which the text-smear
/// ratchet reports at boot. Measured — the default put eight x labels in 380 px
/// of axis, and the radar's radial labels landed on its spoke names.
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurface),
        legend: true,
        x_ticks: 4,
        y_ticks: 4,
        ..ChartStyle::default()
    }
}

/// The radar's style: no radial tick labels at all.
///
/// A radar's rings carry their scale along one spoke, and that spoke also
/// carries a category name — so in a card this size the two are the same pixels.
/// The rings stay; only the numbers go, which is what a radar is read for.
fn radar_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        y_ticks: 0,
        ..chart_style(theme)
    }
}

/// Cached projection: one `(ToggleState, on)` pair per legend entry on the
/// board, in card order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoardState {
    rows: [(ToggleState, bool); N],
}

impl BoardState {
    fn idle() -> Self {
        Self {
            rows: [(ToggleState::Idle, false); N],
        }
    }

    /// The visibility mask for the `LEN` entries starting at `at`.
    fn mask<const LEN: usize>(self, at: usize) -> [bool; LEN] {
        let mut out = [true; LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.rows[at + i].1;
        }
        out
    }

    /// The postures for the `LEN` entries starting at `at`, which a chart cannot
    /// know and this view does.
    fn postures(self, at: usize, len: usize) -> LegendPostures {
        (0..len).fold(LegendPostures::at_rest(), |acc, i| {
            acc.under(i, &self.rows[at + i].0)
        })
    }
}

/// A card: its chart's scene, and a caption derived from that chart's legend.
fn card(chart_scene: Scene, legend: &Legend, rect: Rect, tag: &str, ink: Color) -> Vec<Scene> {
    vec![
        chart_scene,
        Scene::Text(
            TextNode::styled(
                caption(legend),
                Rect::default(),
                TextStyle::new().with_size_px(CAPTION_FONT_PX).with_fg(ink),
            )
            .with_tag(format!("{tag}.caption"))
            .with_layout(LayoutStyle::new().with_absolute_position(rect.x, rect.y + rect.h + 6)),
        ),
    ]
}

/// view-fn (§6.3): pure sync `BoardState -> Scene`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: BoardState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = chart_style(&theme);
    let ink = theme.resolve(ColorRole::OnSurface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let title = Scene::Text(
        TextNode::styled(
            "Four chart kinds — each card's caption is the chart's own answer",
            Rect::default(),
            TextStyle::new().with_size_px(TITLE_FONT_PX).with_fg(ink),
        )
        .with_tag("board.title")
        .with_layout(LayoutStyle::new().with_absolute_position(CARD_GAP, 20)),
    );

    let throughput = throughput(state.mask::<THROUGHPUT_N>(THROUGHPUT_AT));
    let share = share(state.mask::<SHARE_N>(SHARE_AT));
    let profile = profile(state.mask::<PROFILE_N>(PROFILE_AT));
    let sizes = sizes();

    let mut children = vec![title];
    children.extend(card(
        throughput.build(THROUGHPUT_RECT, &style),
        &throughput.legend(),
        THROUGHPUT_RECT,
        THROUGHPUT_TAG,
        muted,
    ));
    children.extend(card(
        share.build(SHARE_RECT, &style),
        &share.legend(),
        SHARE_RECT,
        SHARE_TAG,
        muted,
    ));
    children.extend(card(
        profile.build(PROFILE_RECT, &radar_style(&theme)),
        &profile.legend(),
        PROFILE_RECT,
        PROFILE_TAG,
        muted,
    ));
    children.extend(card(
        sizes.build(SIZES_RECT, &style),
        &sizes.legend(),
        SIZES_RECT,
        SIZES_TAG,
        muted,
    ));

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct ChartLegendsView;

impl WidgetCore for ChartLegendsView {
    type State = BoardState;
    // Every state change flows through `apply_key` (keyboard) or the input
    // router's per-entry pointer dispatch — never the enum `keybinding` channel.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(toggle_group::boot_toggle(BOOT_ON[0]))
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        toggle_group::extra_toggles(&LEGEND_TAGS, &BOOT_ON)
    }

    fn tag() -> &'static str {
        LEGEND_TAGS[0]
    }

    fn read_state(scene: &Scene) -> BoardState {
        let mut out = BoardState::idle();
        for (i, slot) in out.rows.iter_mut().enumerate() {
            *slot = toggle_group::read_toggle(scene, LEGEND_TAGS[i]);
        }
        out
    }

    fn view(state: BoardState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-chart-legends (R1722 §5.38 a chart declares its legend)"
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        toggle_group::apply_key(scene, focused, key, &LEGEND_TAGS)
    }

    fn fmt_state_log(state: &BoardState) -> String {
        state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (s, on))| format!("{i}={}{}", s.as_name(), if *on { "+" } else { "-" }))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl WidgetA11y for ChartLegendsView {
    /// Three `group`s of `button[aria-pressed]`, one per chart that declared the
    /// gesture — **each derived by its own chart**, so the roster a screen
    /// reader walks is the roster that card drew.
    ///
    /// The bar card contributes nothing, and this function does not know that:
    /// its legend names no parts, so it announces no controls.
    fn access_node(state: &BoardState, focused: Option<&str>) -> Vec<AccessNode> {
        let style = ChartStyle::default();
        let mut nodes = throughput(state.mask::<THROUGHPUT_N>(THROUGHPUT_AT)).legend_access_nodes(
            THROUGHPUT_RECT,
            &style,
            &state.postures(THROUGHPUT_AT, THROUGHPUT_N),
            focused,
        );
        nodes.extend(share(state.mask::<SHARE_N>(SHARE_AT)).legend_access_nodes(
            SHARE_RECT,
            &style,
            &state.postures(SHARE_AT, SHARE_N),
            focused,
        ));
        nodes.extend(
            profile(state.mask::<PROFILE_N>(PROFILE_AT)).legend_access_nodes(
                PROFILE_RECT,
                &style,
                &state.postures(PROFILE_AT, PROFILE_N),
                focused,
            ),
        );
        nodes.extend(sizes().legend_access_nodes(
            SIZES_RECT,
            &style,
            &LegendPostures::at_rest(),
            focused,
        ));
        nodes
    }
}

impl WidgetView for ChartLegendsView {
    type Renderer = HelloChartLegendsRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ChartLegendsView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn state_with(on: [bool; N]) -> BoardState {
        let mut s = BoardState::idle();
        for (i, slot) in s.rows.iter_mut().enumerate() {
            slot.1 = on[i];
        }
        s
    }

    /// Render the view inside an `Owner` scope (`use_theme` needs one).
    fn render(on: [bool; N]) -> Scene {
        Owner::new().run(|| view(state_with(on), &Frame::new()))
    }

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        match scene {
            Scene::Container(c) => c.children.iter().find_map(|ch| find(ch, tag)),
            _ => None,
        }
    }

    fn text_of(scene: &Scene, tag: &str) -> String {
        match find(scene, tag) {
            Some(Scene::Text(t)) => t.content.clone(),
            _ => panic!("{tag} is a text node"),
        }
    }

    #[test]
    fn every_tag_this_file_binds_is_one_a_chart_derives() {
        // The binding hazard R1722 closed at the type level, closed again at the
        // consumer: this file spells eight `&'static str` Tab stops, and every
        // one of them must be a tag some chart on the board actually emits. A
        // prefix changed on one side only fails here rather than quietly leaving
        // a legend entry bound to nothing.
        let mut derived: Vec<String> = Vec::new();
        for legend in [
            throughput([true; THROUGHPUT_N]).legend(),
            share([true; SHARE_N]).legend(),
            profile([true; PROFILE_N]).legend(),
            sizes().legend(),
        ] {
            for index in 0..legend.len() {
                derived.push(legend.entry_tag(index));
            }
        }
        derived.sort();
        let mut bound: Vec<String> = LEGEND_TAGS.iter().map(|t| (*t).to_string()).collect();
        bound.sort();
        assert_eq!(bound, derived, "every bound tag is a derived tag, and back");
    }

    #[test]
    fn the_board_asks_and_the_captions_differ_by_what_it_was_told() {
        // Nothing in `caption` knows which chart it is describing; three cards
        // say how many parts they have and the fourth says it has none.
        let scene = render(BOOT_ON);
        assert_eq!(
            text_of(&scene, "throughput.caption"),
            "3 parts — press one to hide it"
        );
        assert_eq!(
            text_of(&scene, "share.caption"),
            "3 parts — press one to hide it"
        );
        assert_eq!(
            text_of(&scene, "profile.caption"),
            "2 parts — press one to hide it"
        );
        assert_eq!(text_of(&scene, "sizes.caption"), "no parts to name");
    }

    #[test]
    fn the_card_that_names_no_parts_offers_no_focus_stop() {
        let scene = render(BOOT_ON);
        for tag in ["throughput.legend.0", "share.legend.0", "profile.legend.0"] {
            let Some(Scene::Container(entry)) = find(&scene, tag) else {
                panic!("{tag} is a focusable entry container")
            };
            assert!(entry.layout.focusable, "{tag} is a Tab stop");
        }
        assert!(
            find(&scene, "sizes.legend.0").is_none(),
            "the bar card has no legend entry to reach"
        );
    }

    /// The path commands of `tag`, or `None` when it is not drawn.
    fn path_of(scene: &Scene, tag: &str) -> Option<Vec<pinion_core::scene::PathCommand>> {
        match find(scene, tag) {
            Some(Scene::Path(p)) => Some(p.commands.clone()),
            _ => None,
        }
    }

    /// Every part shown except the one at `at` in the flat toggle state.
    fn only_hidden(at: usize) -> [bool; N] {
        let mut off = BOOT_ON;
        off[at] = false;
        off
    }

    #[test]
    fn each_card_reads_its_own_run_of_the_flat_toggle_state() {
        // ★ Hiding exactly ONE part, so the cards are distinguishable. An
        // earlier draft of this test turned off the first toggle of two cards
        // at once — which made a card reading its neighbour's run produce the
        // identical scene, and a counterfactual that swapped them passed.
        let line_off = render(only_hidden(THROUGHPUT_AT));
        assert!(
            path_of(&line_off, "throughput.series.0").is_none(),
            "hiding the line's first part hides the line's first part"
        );
        assert!(
            path_of(&line_off, "share.slice.0").is_some(),
            "★ and leaves the DONUT's first slice alone"
        );
        assert!(
            find(&line_off, "profile.series.0").is_some(),
            "★ and the radar's first ring alone"
        );

        let slice_off = render(only_hidden(SHARE_AT));
        assert!(
            path_of(&slice_off, "share.slice.0").is_none(),
            "hiding the donut's first part hides the donut's first part"
        );
        assert!(
            path_of(&slice_off, "throughput.series.0").is_some(),
            "★ and leaves the LINE's first series alone"
        );

        let ring_off = render(only_hidden(PROFILE_AT));
        assert!(
            find(&ring_off, "profile.series.0").is_none(),
            "hiding the radar's first part hides the radar's first part"
        );
        assert!(
            path_of(&ring_off, "throughput.series.0").is_some(),
            "★ and leaves the line alone"
        );
    }

    #[test]
    fn hiding_a_line_leaves_the_other_lines_and_hiding_a_slice_regrows_the_ring() {
        // The two hiding rules, side by side on one screen. A donut's parts must
        // sum to the whole, so hiding one *changes* the others' geometry; a
        // line's must not, so the grid holds still.
        let all_on = render(BOOT_ON);

        let line_off = render(only_hidden(THROUGHPUT_AT));
        assert!(path_of(&all_on, "throughput.series.0").is_some());
        assert!(
            path_of(&line_off, "throughput.series.0").is_none(),
            "the hidden series draws nothing"
        );
        assert_eq!(
            path_of(&all_on, "throughput.series.1"),
            path_of(&line_off, "throughput.series.1"),
            "and its neighbour did not move"
        );

        let slice_off = render(only_hidden(SHARE_AT));
        assert!(path_of(&all_on, "share.slice.0").is_some());
        assert!(
            path_of(&slice_off, "share.slice.0").is_none(),
            "the hidden slice leaves the ring"
        );
        assert_ne!(
            path_of(&all_on, "share.slice.1"),
            path_of(&slice_off, "share.slice.1"),
            "★ and the ring re-normalised around what is left — the other rule"
        );
    }

    #[test]
    fn three_groups_are_announced_and_the_fourth_card_announces_nothing() {
        let nodes = ChartLegendsView::access_node(&state_with(BOOT_ON), None);
        // 3 groups + 8 entries.
        assert_eq!(nodes.len(), 3 + N);
        assert!(
            nodes.iter().any(|n| n.tag == "throughput.legend"),
            "the line card's group"
        );
        assert!(nodes.iter().any(|n| n.tag == "share.legend"));
        assert!(nodes.iter().any(|n| n.tag == "profile.legend"));
        assert!(
            !nodes.iter().any(|n| n.tag.starts_with("sizes.")),
            "the bar card announces no control at all"
        );
    }

    #[test]
    fn an_off_entry_is_announced_off() {
        let mut off = BOOT_ON;
        off[SHARE_AT + 1] = false;
        let nodes = ChartLegendsView::access_node(&state_with(off), None);
        let entry = nodes
            .iter()
            .find(|n| n.tag == "share.legend.1")
            .expect("the slice's entry is announced");
        assert_eq!(entry.state.checked, Some(false));
    }
}
