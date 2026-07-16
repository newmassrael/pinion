//! `hello-chart-fill` — R1360 consumer of `pinion-chart`'s **layout-native**
//! entry point: a chart that FILLS its layout slot and re-scales when the
//! slot resizes.
//!
//! ## What this demonstrates (and why it could not exist before)
//!
//! `hello-chart` pins its chart to a compile-time `const CHART_RECT`, because
//! [`LineChart::build`] resolves everything against a window-absolute rect
//! handed to it before layout runs. That chart cannot flex, dock, or respond
//! to a resize — the target consumer of the whole dataviz axis (a dockable
//! monitoring dashboard) was unreachable *by construction*.
//!
//! Two rounds removed that:
//!
//! * **R1358** made a [`Scene::Path`]'s commands relative to its own `rect`,
//!   so a path is placed by layout instead of being welded to a window
//!   coordinate. That was the *primitive* blocker; nothing chart-side could
//!   have worked around it.
//! * **R1360** added [`LineChart::build_fill`], which authors every child in
//!   the chart's own `(0, 0)..(w, h)` frame and returns a **fill-parent**
//!   root. taffy sizes + places that root; each child's parent-relative
//!   `absolute_position` (R55.D.6) resolves against wherever it lands.
//!
//! ## The measured-rect seam (the reactive half)
//!
//! `build_fill` needs the slot's size *while the view fn runs*, but a slot's
//! size is only known *after* layout. That is exactly the R1012 per-pane
//! shape, so this binding reuses it rather than inventing one:
//!
//! 1. The view reads [`use_pane_viewport_size`] keyed on the chart's own tag.
//!    First paint: `(0, 0)` = unmeasured → `build_fill` emits an empty
//!    fill-parent root (which still *measures*, since its size comes from its
//!    slot, not its content).
//! 2. `compute_layout` sizes that root to the window; the shell's post-layout
//!    `publish_pane_viewports` writes the measured rect to the tag's signal
//!    and returns a dirty bit.
//! 3. The shell's **same-frame re-pass** re-runs `view` + `compute_layout`
//!    once, so the chart paints at the real size on the very first frame —
//!    not one frame late.
//! 4. Every resize repeats 2-3, so dragging the window edge re-scales the
//!    whole chart to the new slot.
//!
//! To be precise about what a resize does and does not change: the DOMAIN is
//! derived from the data (or pinned by a caller), so it is unaffected — the
//! axis still spans the same values and `nice_ticks` still targets the same
//! count. What changes is the plot's pixel rect, hence the scale's range and
//! every vertex's position. Drag the window edge and watch the plot re-scale;
//! the status line prints the measured size the chart last built at.
//!
//! ## Verification
//!
//! The tests below place this view's scene through the real `compute_layout`
//! at two window sizes and assert the chart root is measured to each — the
//! property `hello-chart` cannot have. The reactive publish itself is a
//! live-paint-only seam (the RPC mirror never publishes), so it is exercised
//! by running the binary and resizing, and the shell's own
//! `publish_pane_viewports` tests pin the mechanism.

use pinion_a11y::WidgetA11y;
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, SizeValue, TextStyle};
use pinion_core::widget_core::{ExtraExternal, PrimarySurface};
use pinion_core::{
    ColorRole, External, Frame, Scene, WidgetCore, use_pane_viewport_size, use_theme,
};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

// pinion-forge codegen output — defines `HelloChartFillRenderer` +
// `HelloChartFillRendererError` (the Vello wrapper).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloChartFillRenderer, HelloChartFillRendererError);

/// Opening size only — unlike `hello-chart`'s `CHART_RECT`, nothing about the
/// chart's geometry is derived from these. Drag the window to any size.
const WIN_W: u32 = 720;
const WIN_H: u32 = 420;

/// Shared `ThemeProvider` cache key (the `"app"` gallery convention).
const THEME_TAG: &str = "app";

/// The chart root's tag. It is BOTH the introspection prefix (`chart.series.0`
/// …) and the measured-rect seam's key: the shell publishes the rect of the
/// node carrying this tag, and the view reads it back through
/// `use_pane_viewport_size(CHART_TAG)`. One tag, so the thing measured is by
/// construction the thing painted.
const CHART_TAG: &str = "chart";

/// A deterministic sample feed — 24 buckets of two series. Data is programmatic
/// (user-facing data input is a separate gap); the axis behaviour under resize
/// is what this binding exists to show.
fn sample_series() -> Vec<Series> {
    let ingress: Vec<DataPoint> = (0..24)
        .map(|i| {
            let x = f64::from(i);
            DataPoint::new(x, 900.0 + 620.0 * (x / 3.4).sin() + 38.0 * x)
        })
        .collect();
    let egress: Vec<DataPoint> = (0..24)
        .map(|i| {
            let x = f64::from(i);
            DataPoint::new(x, 500.0 + 300.0 * (x / 5.0).cos() + 22.0 * x)
        })
        .collect();
    vec![
        Series::new("ingress", ingress),
        Series::new("egress", egress),
    ]
}

/// The chart style, resolved from the live theme so the chart tracks
/// light/dark (series colours come from the crate's Okabe-Ito palette and are
/// theme-independent by design).
fn chart_style(theme: &pinion_core::Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        ..ChartStyle::default()
    }
}

/// view-fn (§6.3): pure sync `() -> Scene`.
///
/// The whole point is the middle slot: a flex-grow row that the chart fills.
/// The chart's geometry is derived from the MEASURED size of that slot, never
/// from `WIN_W` / `WIN_H`.
//
// `&Frame` per the §6.3 view-fn signature contract (same allow as the sibling
// bindings; `Frame` is presently a Copy ZST).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    // The measured-rect seam. `(0, 0)` until the shell's post-layout publish
    // lands; the same-frame re-pass then re-runs this view with the real size.
    let (cw, ch) = use_pane_viewport_size(CHART_TAG);

    let chart = LineChart::new(sample_series())
        .filled(true)
        .build_fill((cw, ch), &chart_style(&theme));

    // R1360.4 — AUTO height, not a constant. A definite width with an `auto`
    // height routes the row through the text measure, which wraps at that
    // width and reports the real height; the flex-grow slot below absorbs
    // whatever is left. So the title may be any length and the window any
    // width, and the chrome simply reflows.
    //
    // The first cut pinned this row to a constant and then SHORTENED THE
    // TITLE TEXT to fit — editing the prose to fit the layout, in the one
    // binding whose whole thesis is that geometry must come from layout
    // rather than constants. (It also mis-stated the failure: `TextStyle`
    // defaults to `TextOverflow::Visible` and nothing clips a Container, so
    // an over-long title would have *overflowed onto the chart*, not clipped.)
    let title = Scene::Text(
        TextNode::styled(
            "Throughput (pkt/s) — drag the window edge; the chart re-scales".to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(16)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(
            LayoutStyle::new().with_size(Size::auto().with_width(SizeValue::Percent(100))),
        ),
    );

    // The chart's slot: a flex-grow row. Its height is whatever is left after
    // the title + status rows — i.e. it changes with every window resize, which
    // is exactly the forcing function.
    let slot = Scene::Container(
        ContainerNode::new(vec![chart]).with_layout(
            LayoutStyle::new()
                .with_flex_grow(1.0)
                .with_size(Size::auto().with_width(SizeValue::Percent(100))),
        ),
    );

    let status = Scene::Text(
        TextNode::styled(
            if cw == 0 || ch == 0 {
                "slot unmeasured — the chart paints on the next pass".to_string()
            } else {
                format!("chart slot measured {cw} x {ch} px (built at this size)")
            },
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(
            LayoutStyle::new().with_size(Size::auto().with_width(SizeValue::Percent(100))),
        ),
    );

    // The root MUST paint the theme surface. Nothing else fills the window,
    // so without this the title/status rows and the padding sit on whatever
    // the render surface was cleared to (black on a live window) while the
    // theme hands the text an on-surface colour chosen for that surface —
    // dark text on black, i.e. unreadable. Every sibling binding does this;
    // the fill is the reason their chrome is legible.
    Scene::Container(
        ContainerNode::new(vec![title, slot, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(pinion_core::style::FlexDirection::Column)
                    .with_padding(Rect::new(10, 10, 10, 10))
                    .with_size(
                        Size::auto()
                            .with_width(SizeValue::Percent(100))
                            .with_height(SizeValue::Percent(100)),
                    ),
            ),
    )
}

/// The binding. Hand-written (not `#[widget]`-derived) because the derive
/// macro always emits a primary `create_external`, and this chart is
/// display-only: its sole "input" is the window size, which arrives through
/// the measured-rect seam rather than a pointer. PR-51's `primary_surface()`
/// opt-out is exactly this shape.
struct ChartFillView;

impl WidgetCore for ChartFillView {
    type State = ();
    type Event = ();

    /// (PR-51) No primary surface: no statechart, no captured gesture. The
    /// resize path is the shell's, not an External's.
    fn primary_surface() -> Option<PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("hello-chart-fill has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("hello-chart-fill has no primary surface — see primary_surface()")
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
        "pinion hello-chart-fill (R1360: the chart fills its slot — resize me)"
    }

    fn fmt_state_log(_state: &Self::State) -> String {
        "display-only (the chart's only input is its measured slot)".to_string()
    }
}

// Default a11y surface — the chart's own AT description is `pinion-chart`'s
// (R1359 `describedby_region`); this binding adds no bespoke nodes.
impl WidgetA11y for ChartFillView {}

impl WidgetView for ChartFillView {
    type Renderer = HelloChartFillRenderer;

    /// Resizable on purpose: the resize IS the demo. `OpenResizable` opens at
    /// a comfortable size and lets the user drag it freely smaller.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::OpenResizable {
            size: (WIN_W, WIN_H),
            min: Some((240, 160)),
        }
    }
}

fn main() {
    pinion_shell::run::<ChartFillView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_runtime::{CoreShell, compute_layout};

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        match scene {
            Scene::Container(c) => c.children.iter().find_map(|ch| find(ch, tag)),
            Scene::Scroll(s) => find(&s.content, tag),
            _ => None,
        }
    }

    /// One paint cycle, driving the REAL seam exactly as
    /// `ShellCore::compute_paint_scene_internal` does: run the view under the
    /// shell's `root_owner`, lay it out, publish the post-layout pane rects,
    /// and — when that publish reports dirty — re-run view + layout once (the
    /// same-frame re-pass). Returns the painted scene plus whether the re-pass
    /// fired.
    ///
    /// Nothing here seeds a signal by hand: the measured size the second view
    /// reads is the one layout actually produced from the window size.
    fn paint_cycle(core: &CoreShell<ChartFillView>, w: u32, h: u32) -> (Scene, bool) {
        let mut cache = pinion_text::LayoutCache::new();
        let run_view = || core.root_owner().run(|| view((), &Frame::new()));

        let mut scene = run_view();
        compute_layout(&mut scene, &mut cache, w, h);
        let dirty = core.publish_pane_viewports(&scene);
        if dirty {
            scene = run_view();
            compute_layout(&mut scene, &mut cache, w, h);
        }
        (scene, dirty)
    }

    fn chart_rect(scene: &Scene) -> Rect {
        match find(scene, CHART_TAG) {
            Some(Scene::Container(c)) => c.rect,
            _ => panic!("chart root present"),
        }
    }

    /// The painted extent of the chart BODY — the union of the x-axis and the
    /// series, i.e. geometry `build_fill` derived from the size it was handed.
    ///
    /// Asserting on [`chart_rect`] alone is not enough and this fn exists to
    /// say why: the root is fill-parent, so its rect tracks the slot *however
    /// stale the body inside it is*. A view that ignored the measured size and
    /// built at a constant would still yield a perfectly-tracking root with a
    /// body overflowing it. The body's extent is the only thing that proves the
    /// size actually reached `build_fill`.
    fn body_extent(scene: &Scene) -> Rect {
        let mut acc: Option<Rect> = None;
        let mut push = |r: Rect| {
            acc = Some(match acc {
                None => r,
                Some(a) => a.union(r),
            });
        };
        if let Some(axis) = find(scene, "chart.axis.x") {
            push(axis.rect());
        }
        for i in 0..2 {
            if let Some(s) = find(scene, &format!("chart.series.{i}")) {
                push(s.rect());
            }
        }
        acc.expect("a measured chart paints an axis + series")
    }

    #[test]
    fn first_paint_bootstraps_through_the_measured_rect_seam() {
        // R1360 — the reactive loop's base case, end to end. Pass 1 runs with
        // the seam unmeasured, so the chart paints nothing; its fill-parent
        // root is nonetheless MEASURED (its size is its slot's, not its
        // content's) — and that is what the publish reports, which fires the
        // same-frame re-pass that paints the real chart. A 0x0 root here would
        // mean the seam can never bootstrap.
        let core: CoreShell<ChartFillView> = CoreShell::new();

        // Pass 1 in isolation: what the view produces before any publish.
        let mut first = core.root_owner().run(|| view((), &Frame::new()));
        let mut cache = pinion_text::LayoutCache::new();
        compute_layout(&mut first, &mut cache, 720, 420);
        let unmeasured = chart_rect(&first);
        assert!(
            unmeasured.w > 0 && unmeasured.h > 0,
            "the unmeasured chart root is still measured to its slot, got {unmeasured:?}"
        );
        assert!(
            find(&first, "chart.series.0").is_none(),
            "an unmeasured chart paints no series"
        );

        // The publish closes the loop: dirty ⇒ the shell re-views this frame.
        assert!(
            core.publish_pane_viewports(&first),
            "the post-layout publish reports dirty, which is what triggers the \
             same-frame re-pass"
        );
        let second = core.root_owner().run(|| view((), &Frame::new()));
        let mut second = second;
        compute_layout(&mut second, &mut cache, 720, 420);
        assert!(
            find(&second, "chart.series.0").is_some(),
            "after the publish the chart paints at its measured size — on the \
             SAME frame, not one frame late"
        );
    }

    /// Regression: the root must paint an OPAQUE theme surface.
    ///
    /// Found by a human looking at the live window, not by this suite — the
    /// first cut of this binding filled nothing, so every pixel outside the
    /// chart body was `RGBA(0,0,0,0)`. A compositing desktop renders that
    /// black while the theme, assuming its own surface, picks a dark
    /// on-surface text colour: dark-on-black, unreadable.
    ///
    /// Why it slipped through, stated exactly — an earlier version of this
    /// comment blamed the tooling and was **wrong**, which is worth more
    /// than the fix. The scene-level tests below only ever asked about
    /// geometry, so they were blind. The screenshot was **not**: the capture
    /// preserves alpha verbatim (`encode_rgba8_png` writes
    /// `ColorType::Rgba`), so `RGBA(0,0,0,0)` was sitting in the PNG the
    /// whole time and one `sample_png_points` call would have printed it.
    /// What flattens transparency onto white is an image *viewer* — and
    /// looking at the rendered image is what I did instead of sampling the
    /// pixel. The tools could see it; the observation method could not.
    #[test]
    fn the_root_paints_an_opaque_surface_behind_the_chrome() {
        let core: CoreShell<ChartFillView> = CoreShell::new();
        let (scene, _) = paint_cycle(&core, 720, 420);
        let Scene::Container(root) = &scene else {
            panic!("root Container")
        };
        let fill = root.style.fill;
        assert_eq!(
            fill.a, 0xFF,
            "the root must fill the window with an OPAQUE theme surface — a \
             transparent root (alpha 0, the default) leaves the title and \
             status rows on the compositor's black, under text coloured for a \
             light surface. Got {fill:?}"
        );
        // …and it is the THEME's surface (resolved in the same owner scope the
        // view ran in), so the chrome tracks light/dark instead of hardcoding.
        let want = core.root_owner().run(|| {
            use_theme(THEME_TAG)
                .theme_animated()
                .resolve(ColorRole::Surface)
        });
        assert_eq!(fill, want, "the root fill is the theme's surface role");
    }

    #[test]
    fn the_chart_tracks_its_slot_instead_of_a_const_rect() {
        // The property `hello-chart` structurally cannot have: its chart is
        // pinned to `const CHART_RECT`. Here one unchanged view fn yields two
        // differently-sized charts purely because the window differs — the
        // rect comes from layout.
        let (big, _) = paint_cycle(&CoreShell::<ChartFillView>::new(), 720, 420);
        let (small, _) = paint_cycle(&CoreShell::<ChartFillView>::new(), 420, 260);
        let (b, s) = (chart_rect(&big), chart_rect(&small));
        assert!(
            b.w > s.w && b.h > s.h,
            "the chart root tracks its slot: 720x420 -> {b:?} must exceed 420x260 -> {s:?}"
        );
        assert!(
            b.w <= 720 && b.h < 420,
            "the chart fits inside its window, got {b:?}"
        );
        // …and the BODY tracks it too. Without this the test would pass for a
        // view that ignored the seam: a fill-parent root tracks the slot even
        // with a stale body inside it.
        let (bb, sb) = (body_extent(&big), body_extent(&small));
        assert!(
            bb.w > sb.w,
            "the chart's painted geometry tracks the slot: {bb:?} must be wider \
             than {sb:?} — equal widths mean the view ignored the measured size"
        );
    }

    #[test]
    fn a_resize_re_publishes_and_rebuilds_the_chart() {
        // The live gesture: the same shell paints at one size, then another
        // (a window drag). The second cycle must report dirty — the measured
        // size genuinely changed — and rebuild the chart to the new slot.
        let core: CoreShell<ChartFillView> = CoreShell::new();
        let (first, _) = paint_cycle(&core, 720, 420);
        let before = chart_rect(&first);

        let (resized, dirty) = paint_cycle(&core, 480, 300);
        assert!(
            dirty,
            "a resize changes the measured slot, so the publish must report \
             dirty and re-view — otherwise the chart would paint one frame stale"
        );
        let after = chart_rect(&resized);
        assert!(
            after.w < before.w && after.h < before.h,
            "the chart root followed the resize: {before:?} -> {after:?}"
        );
        assert!(
            find(&resized, "chart.series.0").is_some(),
            "the resized chart still paints its series"
        );
        // The claim in this test's NAME — "rebuilds the chart" — lives here.
        // The body must be re-derived at the new size and stay inside the new
        // root; a view that ignored the seam would leave the old, larger
        // geometry overflowing the shrunk root.
        let body = body_extent(&resized);
        assert!(
            body.x + body.w <= after.x + after.w + 1,
            "the rebuilt body {body:?} stays within the resized root {after:?} \
             — an overflow means the geometry was built at the stale size"
        );
        assert!(
            body_extent(&first).w > body.w,
            "the body itself shrank with the slot: {:?} -> {body:?}",
            body_extent(&first)
        );

        // Steady state: repainting at the SAME size must not report dirty
        // (equality-skip), or every frame would burn a wasted re-pass.
        let (_, dirty_again) = paint_cycle(&core, 480, 300);
        assert!(
            !dirty_again,
            "an unchanged republish is equality-skipped — no re-pass churn"
        );
    }

    #[test]
    fn series_are_placed_inside_the_measured_chart_root() {
        let core: CoreShell<ChartFillView> = CoreShell::new();
        let (scene, _) = paint_cycle(&core, 720, 420);
        let root = chart_rect(&scene);
        let Some(Scene::Path(p)) = find(&scene, "chart.series.0") else {
            panic!("series 0 painted once measured")
        };
        // Layout — not a window constant — put the series where it is.
        assert!(
            p.rect.x >= root.x && p.rect.y >= root.y,
            "series {:?} sits inside the measured chart root {root:?}",
            p.rect
        );
    }
}
