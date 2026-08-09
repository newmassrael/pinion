//! `hello-candlestick` — R1567 §5.38 a datum whose middle landmarks are
//! **unordered**, and an axis that is a reading rather than a second source.
//!
//! The forcing consumer for [`pinion_chart::Candle`] and
//! [`pinion_chart::CandlestickChart`] (the toolkit's candlestick series).
//!
//! ## Why this is not the box plot again
//!
//! R1553 gave the crate a [`Distribution`](pinion_chart::Distribution) and
//! recorded that a candlestick would be its second consumer. It is not. A
//! distribution's five landmarks are totally ordered by construction; a
//! candle's `open` and `close` are ordered against the extremes and **not
//! against each other**, and that absence is the datum. Wednesday here
//! opened at 106 and closed at 106 — a *doji*, the session the whole form is
//! read for — and the toolkit has no name for it: its documented rule paints
//! `increasingColor` only when the close is *higher* than the open, so a doji
//! takes the losing colour and no accessor can say otherwise.
//!
//! ## The direction is drawn twice, on purpose
//!
//! A rising body is **hollow**, a falling one **solid** — the traditional
//! Japanese form, which predates colour. The toolkit encodes the direction in
//! hue alone, and green-and-red is the worst possible pair for the commonest
//! colour-vision deficiency. The "mono" chip here strips the hue to a single
//! ink: the chart stays readable, because the fill carried it all along.
//!
//! ## The axis chips are two readings of ONE dataset
//!
//! The six sessions are Monday to Friday and then the *next* Monday. On the
//! ordinal reading they abut and the weekend is invisible — which is right for
//! reading price action. On the elapsed reading the last gap is three days
//! wide, because three days passed. The toolkit reaches those two pictures by
//! attaching two different axis objects and handing the category axis a string
//! list unrelated to the sets' timestamps; here the slot names are derived
//! from the instants, so the two cannot disagree.
//!
//! ## Verification (substrate-first)
//!
//! `scene/snapshot` exposes the whole schema as tagged data —
//! `chart.candle.{i}` (with its fill alpha and stroke colour, which is how
//! the two encodings are read apart), `chart.wick.{i}.hi` / `.lo`,
//! `chart.cap.{i}.hi` / `.lo`, `chart.xlabel.{i}` / `chart.label.x.{k}`. No
//! pixels are sampled (§2 #1 / §2 #7). See
//! `tools/demos/r1567_candle_direction_is_the_datum.py`.

use pinion_a11y::{
    AccessFocus, AccessNode, AriaRole, RadioCell, ToggleSegment, WidgetA11y,
    radiogroup_radio_nodes, toggle_button_group_nodes,
};
use pinion_chart::{Candle, CandlestickChart, ChartStyle, SessionAxis};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::widgets::toggle_group;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::chip::{CHIP_HEIGHT, option_chip};
use pinion_widget_paint::radio_composite as rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloCandlestickRenderer, HelloCandlestickRendererError);

const WIN_W: u32 = 860;
const WIN_H: u32 = 520;
const THEME_TAG: &str = "app";

/// The x-axis reading radio group (PRIMARY external). Its cells are
/// `reading#<i>`, which the R51.42 `'#'`-split routes back to this one
/// coordinator.
const READING_TAG: &str = "reading";

/// The caps and monochrome toggles (extras) — independent switches, each its
/// own Tab stop, unlike the radio group's single roving stop.
const CAPS_TAG: &str = "caps";
const MONO_TAG: &str = "mono";

/// The WAI-ARIA `group` label parent for the two option toggles.
const OPTIONS_GROUP_TAG: &str = "options";

/// The caption's tag — the SSOT for the painted text and for the demo.
const CAPTION_TAG: &str = "caption";

/// The two x-axis readings, in the order their chips appear.
const READINGS: [SessionAxis; 2] = [SessionAxis::Ordinal, SessionAxis::Elapsed];

/// Chip labels — the toolkit's axis class names in parentheses, so a reader
/// can look the equivalent up.
const READING_LABELS: [&str; 2] = ["Sessions (category)", "Elapsed (datetime)"];

/// Boot: the ordinal reading (what a price chart is read on), no caps, hue
/// on — so the first interaction with any chip is a visible change.
const BOOT_READING: usize = 0;
const BOOT_CAPS: bool = false;
const BOOT_MONO: bool = false;

const TITLE_FONT_PX: u32 = 17;
const CAPTION_FONT_PX: u32 = 12;
const CHIP_W: u32 = 156;

/// Window-absolute plot region. The chart must be handed its final geometry
/// before layout runs (see the `pinion-chart` coordinate contract).
const CHART_RECT: Rect = Rect::new(16, 96, WIN_W - 32, WIN_H - 180);

/// 2026-03-02T00:00:00Z — a Monday, so the fixture's weekend gap is real.
const MON: f64 = 1_772_409_600_000.0;
const DAY_MS: f64 = 86_400_000.0;

/// The session that closed exactly where it opened.
const DOJI: usize = 3;

/// Six daily sessions: Monday to Friday, then the NEXT Monday.
///
/// `(day offset, open, high, low, close)`. Day 5 and 6 are absent because the market was shut, which is the whole
/// reason the two x-axis readings exist — and day 3 closes where it opened,
/// which is the *doji* the toolkit cannot name.
const SESSIONS: [(f64, f64, f64, f64, f64); 6] = [
    (0.0, 100.0, 104.0, 99.0, 103.0),
    (1.0, 103.0, 105.0, 101.0, 102.0),
    (2.0, 102.0, 106.0, 102.0, 106.0),
    (3.0, 106.0, 107.0, 103.0, 106.0),
    (4.0, 106.0, 108.0, 104.0, 105.0),
    (7.0, 105.0, 111.0, 105.0, 110.0),
];

/// The six sessions as data.
///
/// The ONE construction the chart, the caption and the tests all read, so a
/// caption that disagreed with the plot would be a bug in the crate rather
/// than in a second copy of the numbers here.
fn sessions() -> Vec<Candle> {
    SESSIONS
        .iter()
        .map(|&(d, o, h, l, c)| {
            Candle::new(d.mul_add(DAY_MS, MON), o, h, l, c)
                .expect("every fixture session is well ordered")
        })
        .collect()
}

/// The chart for one set of options — the ONE place the reading, the caps and
/// the monochrome choice are applied.
fn chart_for(state: &Options) -> CandlestickChart {
    let chart = CandlestickChart::new(sessions()).with_caps(state.caps);
    let chart = match state.reading() {
        SessionAxis::Ordinal => chart.ordinal(),
        SessionAxis::Elapsed => chart.elapsed(),
    };
    if state.mono {
        // One ink for all three directions: the hue channel carries nothing.
        // What a reader is left with is the FILL, which is the point.
        let ink = Color::rgb(0x33, 0x38, 0x42);
        chart.with_direction_colors(ink, ink, ink)
    } else {
        chart
    }
}

/// The themed chart style.
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurface),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        label_size_px: 12,
        y_ticks: 6,
        ..ChartStyle::default()
    }
}

/// The caption: what this reading shows, and what the hue channel is worth.
///
/// Every clause is read back off the built chart rather than restated, so
/// none can drift from the picture.
fn caption(state: &Options) -> String {
    let chart = chart_for(state);
    let doji = &chart.candles()[DOJI];
    let head = format!(
        "Wed closed where it opened \u{2014} a {} ({} body, change {:+.2}). ",
        doji.direction().name(),
        doji.direction().body_fill().name(),
        doji.change(),
    );
    let axis = match state.reading() {
        SessionAxis::Ordinal => {
            "Session axis \u{2014} the six bars abut, so the weekend between Fri \
             and Mon takes no width: price action, not elapsed time."
        }
        SessionAxis::Elapsed => {
            "Elapsed axis \u{2014} the same six bars over real UTC time, so the \
             weekend is three days of empty plot. One datum, two readings."
        }
    };
    let ratio = chart.direction_contrast();
    let hue = if state.mono {
        format!(
            " Hue removed (rising/falling contrast {ratio:.2}:1) \u{2014} the \
             direction is still readable, because the body fill carries it."
        )
    } else {
        format!(
            " Rising vs falling contrast {ratio:.2}:1; the conventional \
             mid-green/mid-red pair measures 1.11:1, which is why these are \
             separated in luminance."
        )
    };
    format!("{head}{axis}{hue}")
}

/// The reading radio strip: two chips inside one focusable container, which
/// is the group's single Tab stop.
fn reading_row(state: &Options, theme: &Theme) -> Scene {
    let chips: Vec<Scene> = (0..READINGS.len())
        .map(|i| {
            option_chip(
                format!("{READING_TAG}#{i}"),
                READING_LABELS[i],
                state.reading_rows[i].1,
                false,
                CHIP_W,
                state.reading_rows[i].0,
                theme,
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(chips)
            .with_tag(READING_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8)
                    .with_focusable(true)
                    .with_absolute_position(16, 48)
                    .with_size(Size::px(CHIP_W * 2 + 8, CHIP_HEIGHT)),
            ),
    )
}

/// The two option toggles, each its own Tab stop.
fn option_row(state: &Options, theme: &Theme) -> Scene {
    let caps = option_chip(
        CAPS_TAG.to_string(),
        "caps",
        state.caps,
        true,
        88,
        state.caps_row.0,
        theme,
    );
    let mono = option_chip(
        MONO_TAG.to_string(),
        "no hue",
        state.mono,
        true,
        104,
        state.mono_row.0,
        theme,
    );
    Scene::Container(
        ContainerNode::new(vec![caps, mono])
            .with_tag(OPTIONS_GROUP_TAG.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(8)
                    .with_absolute_position(WIN_W - 216, 48)
                    .with_size(Size::px(200, CHIP_HEIGHT)),
            ),
    )
}

/// view-fn (§6.3): pure sync `Options -> Scene`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the `&Frame` shape is the WidgetCore::view signature this wraps"
)]
fn view(state: Options, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(
        TextNode::styled(
            "Daily sessions \u{2014} open / high / low / close, and which way each went",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 18)),
    );

    let chart = chart_for(&state).build(CHART_RECT, &chart_style(&theme));

    let caption = Scene::Text(
        TextNode::styled(
            caption(&state),
            Rect::default(),
            TextStyle::new()
                .with_size_px(CAPTION_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(CAPTION_TAG.to_string())
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(18, WIN_H - 72)
                .with_size(Size::px(WIN_W - 36, 64)),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![
            chart,
            title,
            reading_row(&state, &theme),
            option_row(&state, &theme),
            caption,
        ])
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_size(Size::px(WIN_W, WIN_H)),
        ),
    )
}

/// Which x-axis reading is selected, and whether each option is on.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Options {
    /// Per-reading `(interaction state, selected)`.
    reading_rows: [(RadioState, bool); 2],
    /// The selected reading index (the radio group's 1-of-N invariant).
    reading_selected: usize,
    /// The AT-side roving descendant of the reading group.
    reading_focused: Option<usize>,
    caps_row: (ToggleState, bool),
    mono_row: (ToggleState, bool),
    caps: bool,
    mono: bool,
}

impl Options {
    fn idle() -> Self {
        Self {
            reading_rows: [(RadioState::Idle, false); 2],
            reading_selected: BOOT_READING,
            reading_focused: None,
            caps_row: (ToggleState::Idle, BOOT_CAPS),
            mono_row: (ToggleState::Idle, BOOT_MONO),
            caps: BOOT_CAPS,
            mono: BOOT_MONO,
        }
    }

    /// The selected x-axis reading. An out-of-range index (which the group
    /// cannot produce) falls back to the boot reading rather than panicking
    /// in a view-fn.
    fn reading(&self) -> SessionAxis {
        READINGS
            .get(self.reading_selected)
            .copied()
            .unwrap_or(READINGS[BOOT_READING])
    }
}

struct CandlestickView;

impl WidgetCore for CandlestickView {
    type State = Options;
    // Every change arrives through `apply_key` or the input router's per-chip
    // pointer dispatch — never the enum keybinding channel.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        use pinion_core::widgets::radio::RadioEvent;
        let mut group = RadioGroupExternal::new(READINGS.len());
        group.send(BOOT_READING, RadioEvent::PointerEnter);
        group.send(BOOT_READING, RadioEvent::PointerDown);
        group.send(BOOT_READING, RadioEvent::PointerUp);
        group.send(BOOT_READING, RadioEvent::PointerLeave);
        Box::new(group)
    }

    /// The two option toggles. Built by hand rather than through
    /// `toggle_group::extra_toggles`, which skips index 0 on the assumption
    /// that the first toggle is the primary external — here the primary is
    /// the reading radio group.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(
                CAPS_TAG,
                Box::new(toggle_group::boot_toggle(BOOT_CAPS)) as Box<dyn External>,
            ),
            ExtraExternal::new(
                MONO_TAG,
                Box::new(toggle_group::boot_toggle(BOOT_MONO)) as Box<dyn External>,
            ),
        ]
    }

    fn tag() -> &'static str {
        READING_TAG
    }

    fn read_state(scene: &Scene) -> Options {
        let mut out = Options::idle();
        if let Some(node) = scene.find_external_with_tag(READING_TAG)
            && let Some(intro) = node.handle.introspect()
        {
            rc::read_rows(intro, &mut out.reading_rows);
            out.reading_focused = rc::focused_index(intro);
            out.reading_selected = rc::selected_index(intro).unwrap_or(BOOT_READING);
        }
        out.caps_row = toggle_group::read_toggle(scene, CAPS_TAG);
        out.mono_row = toggle_group::read_toggle(scene, MONO_TAG);
        out.caps = out.caps_row.1;
        out.mono = out.mono_row.1;
        out
    }

    fn view(state: Options, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-candlestick (R1567 §5.38 the middle landmarks are unordered)"
    }

    /// Two keymaps, one per control. `toggle_group::apply_key` returns
    /// `false` unless one of the option tags owns focus, and the reading
    /// branch returns `false` unless [`READING_TAG`] does, so exactly one can
    /// consume a key.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if toggle_group::apply_key(scene, focused, key, &[CAPS_TAG, MONO_TAG]) {
            return true;
        }
        if focused != Some(READING_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(READING_TAG) else {
            return false;
        };
        let Some(idx) = resolve_reading_target(node.handle.introspect(), key) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        rc::drive_activate(intro, idx);
        true
    }

    fn fmt_state_log(state: &Options) -> String {
        format!(
            "{}{}{}",
            state.reading().name(),
            if state.caps { " caps" } else { "" },
            if state.mono { " mono" } else { " hue" }
        )
    }
}

/// The roving-cursor keymap of the reading group.
fn resolve_reading_target(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    let n = READINGS.len();
    match key {
        "ArrowRight" | "ArrowDown" => Some(rc::step(intro, 1, n)),
        "ArrowLeft" | "ArrowUp" => Some(rc::step(intro, -1, n)),
        "Home" => Some(0),
        "End" => Some(n - 1),
        _ => None,
    }
}

impl WidgetA11y for CandlestickView {
    /// The reading group as a WAI-ARIA `radiogroup`, the options as a `group` of `button[aria-pressed]`, and
    /// the caption as a live region — so the doji and the contrast ratio are
    /// HEARD, not only seen. The toolkit's charts implement no accessibility
    /// interface at all.
    fn access_node(state: &Options, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(READING_TAG);
        let active = rc::active_index(&state.reading_rows, state.reading_focused);
        let tags: Vec<String> = (0..READINGS.len())
            .map(|i| format!("{READING_TAG}#{i}"))
            .collect();
        let cells: Vec<RadioCell<'_>> = (0..READINGS.len())
            .map(|i| RadioCell {
                tag: &tags[i],
                label: Some(READING_LABELS[i]),
                state: state.reading_rows[i].0,
                selected: state.reading_rows[i].1,
                focused: group_focused && i == active,
            })
            .collect();
        let mut nodes = radiogroup_radio_nodes(READING_TAG, "X-axis reading", &cells);

        let segments = [
            ToggleSegment {
                tag: CAPS_TAG,
                label: "caps",
                state: state.caps_row.0,
                on: state.caps,
            },
            ToggleSegment {
                tag: MONO_TAG,
                label: "no hue",
                state: state.mono_row.0,
                on: state.mono,
            },
        ];
        nodes.extend(toggle_button_group_nodes(
            OPTIONS_GROUP_TAG,
            "Plot options",
            &segments,
            focused,
        ));

        nodes.push(AccessNode::new(CAPTION_TAG, AriaRole::Status).with_name(caption(state)));
        nodes
    }

    /// R1518 §5.40 — the reading group is one Tab stop with a roving cursor,
    /// so name the radio that cursor addresses as the
    /// `aria-activedescendant`.
    fn access_focus_target(state: &Options, focused: Option<&str>) -> Option<AccessFocus> {
        rc::composite_focus_target(
            READING_TAG,
            focused,
            rc::active_index(&state.reading_rows, state.reading_focused),
        )
    }
}

impl WidgetView for CandlestickView {
    type Renderer = HelloCandlestickRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<CandlestickView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_chart::Direction;
    use pinion_core::Owner;

    /// The session after the weekend — the one the readings disagree about.
    const AFTER_GAP: usize = 5;

    fn options(reading: usize, caps: bool, mono: bool) -> Options {
        let mut out = Options::idle();
        out.reading_selected = reading;
        out.reading_rows[reading].1 = true;
        out.caps = caps;
        out.caps_row = (ToggleState::Idle, caps);
        out.mono = mono;
        out.mono_row = (ToggleState::Idle, mono);
        out
    }

    fn render(reading: usize, caps: bool, mono: bool) -> Scene {
        Owner::new().run(|| view(options(reading, caps, mono), &Frame::new()))
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

    fn count_prefix(scene: &Scene, prefix: &str) -> usize {
        let mut n = usize::from(scene.tag().is_some_and(|t| t.starts_with(prefix)));
        if let Scene::Container(c) = scene {
            for ch in &c.children {
                n += count_prefix(ch, prefix);
            }
        }
        n
    }

    fn text_of(scene: &Scene, tag: &str) -> String {
        match find(scene, tag) {
            Some(Scene::Text(t)) => t.content.clone(),
            _ => panic!("no text node tagged {tag}"),
        }
    }

    /// The `(fill alpha, stroke rgb)` of session `i`'s body — the two
    /// channels the direction is encoded in.
    fn body_ink(scene: &Scene, i: usize) -> (u8, (u8, u8, u8)) {
        let Some(Scene::Path(p)) = find(scene, &format!("chart.candle.{i}")) else {
            panic!("body {i} is a path")
        };
        let fill = p.style.fill.expect("a body declares a fill");
        let s = p.style.stroke.expect("a body declares a stroke");
        (fill.a, (s.color.r, s.color.g, s.color.b))
    }

    /// ★ The round, read off the data: Wednesday closed exactly where it
    /// opened. The toolkit's documented rule paints `increasingColor` only when the close is
    /// HIGHER than the open, so this session takes the losing colour there and
    /// no accessor can say otherwise.
    #[test]
    fn r1567_the_doji_is_its_own_direction() {
        let c = &sessions()[DOJI];
        assert_eq!(c.direction(), Direction::Doji);
        assert!(c.body_height().abs() < f64::EPSILON);
        // The counterfactual is the neighbours: one rose, one fell.
        assert_eq!(sessions()[2].direction(), Direction::Rising);
        assert_eq!(sessions()[4].direction(), Direction::Falling);
    }

    /// ★ The direction reaches the paint TWICE, and the fill half is the one
    /// that survives losing the hue. With "no hue" on, all three hues are one
    /// ink and the chart is still readable.
    #[test]
    fn r1567_the_direction_survives_the_loss_of_hue() {
        let hued = render(0, false, false);
        let mono = render(0, false, true);

        let coloured: Vec<(u8, u8, u8)> =
            (0..SESSIONS.len()).map(|i| body_ink(&hued, i).1).collect();
        assert!(
            coloured.iter().any(|h| *h != coloured[0]),
            "with hue on the strokes differ: {coloured:?}"
        );
        let single: Vec<(u8, u8, u8)> = (0..SESSIONS.len()).map(|i| body_ink(&mono, i).1).collect();
        assert!(
            single.iter().all(|h| *h == single[0]),
            "with hue off every stroke is one ink: {single:?}"
        );

        // ...and the fill alphas are UNCHANGED, which is the claim.
        let alphas =
            |s: &Scene| -> Vec<u8> { (0..SESSIONS.len()).map(|i| body_ink(s, i).0).collect() };
        assert_eq!(alphas(&hued), alphas(&mono));
        assert_eq!(alphas(&hued), vec![0, 255, 0, 255, 255, 0]);
    }

    /// ★ One dataset, two readings. On the session axis the six bars abut;
    /// on the elapsed axis the weekend is three days wide.
    #[test]
    fn r1567_the_reading_chip_changes_what_the_gap_means() {
        let centre = |scene: &Scene, i: usize| {
            let Some(Scene::Path(p)) = find(scene, &format!("chart.candle.{i}")) else {
                panic!("body {i} present")
            };
            f64::from(p.rect.x) + f64::from(p.rect.w) / 2.0
        };
        let gaps = |scene: &Scene| {
            let px: Vec<f64> = (0..SESSIONS.len()).map(|i| centre(scene, i)).collect();
            px.windows(2).map(|w| w[1] - w[0]).collect::<Vec<f64>>()
        };

        let ordinal = gaps(&render(0, false, false));
        assert!(
            (ordinal[AFTER_GAP - 1] - ordinal[0]).abs() < 2.0,
            "the weekend takes no extra width: {ordinal:?}"
        );

        let elapsed = gaps(&render(1, false, false));
        assert!(
            elapsed[AFTER_GAP - 1] > elapsed[0] * 2.5,
            "the weekend is three days wide: {elapsed:?}"
        );
    }

    /// ★ The x labels change KIND with the reading, because the two answer
    /// different questions: one per session, or one per time tick.
    #[test]
    fn r1567_each_reading_labels_what_it_is_made_of() {
        let ordinal = render(0, false, false);
        assert_eq!(count_prefix(&ordinal, "chart.xlabel."), SESSIONS.len());
        assert_eq!(count_prefix(&ordinal, "chart.label.x."), 0);
        assert_eq!(text_of(&ordinal, "chart.xlabel.5"), "Mar 09");

        let elapsed = render(1, false, false);
        assert_eq!(count_prefix(&elapsed, "chart.xlabel."), 0);
        assert!(count_prefix(&elapsed, "chart.label.x.") > 0);
    }

    /// ★ Every session draws a body and two wicks; the caps are opt-in, as
    /// the toolkit's `capsVisible` is.
    #[test]
    fn r1567_the_caps_chip_adds_two_marks_per_session() {
        let plain = render(0, false, false);
        assert_eq!(count_prefix(&plain, "chart.candle."), SESSIONS.len());
        assert_eq!(count_prefix(&plain, "chart.wick."), SESSIONS.len() * 2);
        assert_eq!(count_prefix(&plain, "chart.cap."), 0);

        let capped = render(0, true, false);
        assert_eq!(count_prefix(&capped, "chart.cap."), SESSIONS.len() * 2);
    }

    /// ★ The caption states the doji, the reading and the contrast — read off
    /// the built chart, so none of it can drift from the picture.
    #[test]
    fn r1567_the_caption_reports_the_chart_it_drew() {
        let text = text_of(&render(0, false, false), CAPTION_TAG);
        assert!(text.contains("a doji"), "{text}");
        assert!(text.contains("solid body"), "{text}");
        assert!(text.contains("Session axis"), "{text}");
        assert!(text.contains("1.11:1"), "{text}");

        let text = text_of(&render(1, false, false), CAPTION_TAG);
        assert!(text.contains("Elapsed axis"), "{text}");

        let text = text_of(&render(0, false, true), CAPTION_TAG);
        assert!(text.contains("Hue removed"), "{text}");
        assert!(text.contains("1.00:1"), "one ink has no contrast: {text}");
    }

    /// ★ The reading group is one Tab stop with two hit targets; the two
    /// options are their own stops.
    #[test]
    fn r1567_the_controls_carry_the_right_tab_stops() {
        let scene = render(0, false, false);
        let Some(Scene::Container(group)) = find(&scene, READING_TAG) else {
            panic!("the reading group is a container")
        };
        assert!(group.layout.focusable, "the group owns the stop");
        for i in 0..READINGS.len() {
            let Some(Scene::Container(cell)) = find(&scene, &format!("{READING_TAG}#{i}")) else {
                panic!("cell {i} is a container")
            };
            assert!(!cell.layout.focusable, "a radio cell is not its own stop");
        }
        for tag in [CAPS_TAG, MONO_TAG] {
            let Some(Scene::Container(c)) = find(&scene, tag) else {
                panic!("{tag} is a container")
            };
            assert!(c.layout.focusable, "{tag} is its own stop");
        }
    }

    /// ★ The doji and the contrast reach assistive technology: the caption is
    /// a live region, the readings a radiogroup, the options pressed buttons.
    #[test]
    fn r1567_a11y_exposes_the_reading_the_options_and_the_report() {
        let nodes = CandlestickView::access_node(&options(1, false, true), None);
        let radios = nodes
            .iter()
            .filter(|n| matches!(n.role, AriaRole::RadioButton))
            .count();
        assert_eq!(radios, READINGS.len(), "one radio per x-axis reading");
        let buttons = nodes
            .iter()
            .filter(|n| matches!(n.role, AriaRole::Button))
            .count();
        assert_eq!(buttons, 2, "one aria-pressed button per option");
        let status = nodes
            .iter()
            .find(|n| matches!(n.role, AriaRole::Status))
            .expect("the caption is a live region");
        let name = status.name.clone().unwrap_or_default();
        assert!(name.contains("a doji"), "names the doji: {name}");
        assert!(name.contains("Elapsed axis"), "names the reading: {name}");
        assert!(name.contains("Hue removed"), "names the ink: {name}");
    }
}
