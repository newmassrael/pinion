//! `hello-path` — R721 §5.16 consumer of the vector-path rendering
//! substrate (the first [`Scene::Path`] paint).
//!
//! ## What this demonstrates
//!
//! Before R721 the [`Scene::Path`] node carried a fully-defined data
//! model — `commands: Vec<PathCommand>`, a `PathStyle { stroke, fill }`
//! sidecar, a §5.16 R682 paint-cache hash, and `scene/snapshot`
//! serialization — but the Vello `paint_adapter` dropped it on the
//! floor (a documented no-op, "Path paint attaches in a follow-up
//! round"). R721 lowers `commands` to a Vello `BezPath` and fills /
//! strokes it, so a path finally rasterizes. This binding paints four
//! paths that, between them, exercise every [`PathCommand`] variant and
//! both [`PathStyle`] arms:
//!
//! * a **filled triangle** (`tag = "tri"`) — `MoveTo` + two `LineTo` +
//!   `Close`, fill-only (non-zero winding);
//! * a **stroked chevron** (`tag = "chevron"`) — `MoveTo` + two
//!   `LineTo`, stroke-only with a round cap (straight segments so the
//!   live-pixel guard can predict a point on the stroke centreline);
//! * a **cubic-Bezier arc** (`tag = "arc"`) — `MoveTo` + `CurveTo`,
//!   stroke-only (the `CurveTo` lowering exercised in real paint,
//!   verified structurally via `scene/snapshot`);
//! * a **demo diamond** (`tag = "demo_path"`) — `MoveTo` + three
//!   `LineTo` + `Close`, whose [`PathStyle`] the Toggle switches
//!   between *fill-only* (Off) and *fill + stroke* (On), proving the
//!   combined arm and that the §5.16 paint-cache re-keys when a
//!   `PathStyle` changes (the `Hash for PathNode` folds the style in).
//!
//! R722 §5.50 adds two **gradient-fill paths** ([`PathStyle::gradient`],
//! isomorphic to `BoxStyle::gradient`): a `grad_linear` rectangle with a
//! 3-stop horizontal ramp (its white midpoint is an exact-stop pixel no
//! solid fill could produce) and a `grad_radial` diamond whose centre
//! pixel is the radial centre stop.
//!
//! Every path is authored in its own box — R1358 made path commands
//! relative to the node's `rect` — and pinned to a fixed window
//! coordinate with `LayoutStyle::absolute_position` (see
//! [[absolute-position-via-layoutstyle]]), which is what keeps the
//! live-pixel anchors deterministic. Shape and placement are separate
//! here: each vertex set below is 0-based, and moving a shape means
//! changing only its `origin` argument.
//!
//! ## Why a Toggle
//!
//! A vector-graphic gallery is intrinsically stateless, but the
//! `AppShell` drives a `WidgetView` with a statechart `External`.
//! Rather than invent a one-off widget ([[abstraction-needs-second-
//! consumer]]), the binding reuses the §5.38 [`ToggleExternal`] purely
//! as the *mode bit*: the Off/On sidecar selects fill-only vs
//! fill+stroke for the demo diamond. The Toggle is the RPC-drivable,
//! AT-exposed control; the paths are the substrate under test.
//!
//! ## Verification (substrate-first)
//!
//! * `scene/snapshot` exposes every path as structured data — the
//!   `commands` array and the `style` sidecar — so
//!   `tools/demos/r721_path.py` reads back each path's command stream
//!   and fill / stroke without OCR (§2 #7 scene-as-data,
//!   [[ai-first-rpc-introspection-obligation]]).
//! * `tools/demos/r721_path_pixel.py` captures the live window and
//!   samples the rendered triangle interior, the chevron stroke, and
//!   the diamond centre (which flips colour with the Toggle) — the
//!   live-pixel guard a structural query cannot replace
//!   ([[introspection-from-paint-not-screen]], R706/R707.3 precedent).

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, Gradient, JustifyContent, LayoutStyle, PathStyle,
    Size, Stroke, StrokeCap, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output: `pub struct HelloPathRenderer` +
// `HelloPathRendererError` + async `new<...>` + sync `render` /
// `resize`. R46.3.3 emit template uses fully-qualified `::vello::*`
// paths so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can build /
// render / resize it. Identical pattern to hello-gradient.
vello_renderer_impl!(HelloPathRenderer, HelloPathRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 400;

const THEME_TAG: &str = "app";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;
const SWITCH_FONT_PX: u32 = 14;

/// Material 3 state-layer overlay weights (linear-sRGB lerp toward
/// [`ColorRole::OnSurface`]) for the switch chrome — 8 % hover / 12 %
/// pressed, matching the rest of the widget gallery.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

// ── Fixed path colours ─────────────────────────────────────────────
// Theme-independent so the live-pixel guard can predict the exact RGB
// at each anchor (the paths are the substrate under test; the theme is
// orthogonal chrome). `PATH_BLUE` fills the triangle and the Off-mode
// diamond; `DEMO_ON_FILL` is the On-mode diamond fill; `DEMO_ON_STROKE`
// its added outline; `STROKE_TEAL` strokes the chevron and the arc.
const PATH_BLUE: Color = Color::rgb(0x21, 0x96, 0xf3);
const DEMO_ON_FILL: Color = Color::rgb(0xe5, 0x39, 0x35);
const DEMO_ON_STROKE: Color = Color::rgb(0x10, 0x10, 0x10);
const STROKE_TEAL: Color = Color::rgb(0x00, 0x96, 0x88);

const STROKE_W: u32 = 8;
const DEMO_STROKE_W: u32 = 8;

// ── R722 gradient-path fill colours (fixed, theme-independent) ──────
// `GRAD_A` / `GRAD_B` are the gradient endpoints; `GRAD_MID` is the
// midpoint stop of the linear ramp — an exact-stop pixel anchor that no
// solid fill or stroke could produce (the colour AT a stop is the stop
// colour regardless of interpolation space, per the R708 lesson).
const GRAD_A: Color = Color::rgb(0x21, 0x96, 0xf3); // blue
const GRAD_B: Color = Color::rgb(0xe5, 0x39, 0x35); // red
const GRAD_MID: Color = Color::rgb(0xff, 0xff, 0xff); // white

// R1358 — every vertex set below is authored RELATIVE to its own node's
// rect (`(0, 0)` = the node's top-left); `path_node`'s `origin` places it.
// The window coordinates quoted as live-pixel anchors are `origin + vertex`,
// which is exactly where the paint adapter's `translate(rect.{x,y})` puts
// them.

/// Triangle vertices, rect-relative to the `tri` node at (50, 80).
/// Centroid ≈ (90, 133) in window px — the fill-arm live-pixel anchor.
const TRI: [(f32, f32); 3] = [(0.0, 80.0), (80.0, 80.0), (40.0, 0.0)];

/// Chevron polyline vertices, rect-relative to the `chevron` node at
/// (160, 90). The midpoint of the first segment, (185, 120) in window px,
/// lies on the stroke centreline — the stroke-arm live-pixel anchor.
const CHEVRON: [(f32, f32); 3] = [(0.0, 60.0), (50.0, 0.0), (100.0, 60.0)];

/// Diamond vertices, rect-relative to the `demo_path` node at (110, 230).
/// Centre (160, 270) in window px is the Toggle-witness anchor:
/// `PATH_BLUE` (Off) vs `DEMO_ON_FILL` (On).
const DIAMOND: [(f32, f32); 4] = [(50.0, 0.0), (100.0, 40.0), (50.0, 80.0), (0.0, 40.0)];

fn pt(p: (f32, f32)) -> PathPoint {
    PathPoint::new(p.0, p.1)
}

/// A closed polygon command stream: `MoveTo` the first vertex, `LineTo`
/// each subsequent vertex, then `Close`.
fn closed_polygon(verts: &[(f32, f32)]) -> Vec<PathCommand> {
    let mut cmds = Vec::with_capacity(verts.len() + 2);
    let mut iter = verts.iter();
    if let Some(&first) = iter.next() {
        cmds.push(PathCommand::MoveTo(pt(first)));
        for &v in iter {
            cmds.push(PathCommand::LineTo(pt(v)));
        }
        cmds.push(PathCommand::Close);
    }
    cmds
}

/// An open polyline command stream: `MoveTo` the first vertex, `LineTo`
/// each subsequent vertex, no `Close`.
fn open_polyline(verts: &[(f32, f32)]) -> Vec<PathCommand> {
    let mut cmds = Vec::with_capacity(verts.len());
    let mut iter = verts.iter();
    if let Some(&first) = iter.next() {
        cmds.push(PathCommand::MoveTo(pt(first)));
        for &v in iter {
            cmds.push(PathCommand::LineTo(pt(v)));
        }
    }
    cmds
}

/// A single cubic-Bezier arch: `MoveTo` the left foot, `CurveTo` up over
/// two control points to the right foot. Rect-relative to the `arc` node
/// at (280, 80), so the arch spans its own 60x60 box corner to corner.
fn arc_commands() -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo(pt((0.0, 60.0))),
        PathCommand::CurveTo {
            c1: pt((0.0, 0.0)),
            c2: pt((60.0, 0.0)),
            end: pt((60.0, 60.0)),
        },
    ]
}

/// Build an absolutely-positioned [`Scene::Path`]. `origin` / `size` place
/// the node; its `commands` are authored relative to that rect (R1358), so
/// moving a shape means changing `origin` alone — the geometry is untouched.
fn path_node(
    tag: &'static str,
    origin: (u32, u32),
    size: (u32, u32),
    commands: Vec<PathCommand>,
    style: PathStyle,
) -> Scene {
    Scene::Path(
        PathNode::new(Rect::default(), commands, style)
            .with_tag(tag)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(origin.0, origin.1)
                    .with_size(Size::px(size.0, size.1)),
            ),
    )
}

/// R722 §5.50 gradient-fill paths — the first [`PathStyle::gradient`]
/// consumers (isomorphic to `BoxStyle::gradient`, R708). Two static,
/// theme-independent shapes so the live-pixel guard predicts exact RGB:
///
/// * `grad_linear` — a rectangle path with a 3-stop horizontal ramp
///   (`GRAD_A` → `GRAD_MID` → `GRAD_B`). Its midpoint pixel is the
///   white mid stop, which is impossible for any solid fill or stroke;
/// * `grad_radial` — a diamond path with a 2-stop radial gradient
///   (`GRAD_B` centre → `GRAD_A` edge). Its exact centre pixel is the
///   centre stop colour.
///
/// Both are introspectable as data via `scene/snapshot` (geometry kind,
/// stops, UV) and verified by the R722 demo.
fn gradient_paths() -> Vec<Scene> {
    // Linear: a filled rect so any column samples the ramp. R1358 — the
    // geometry fills the node's rect corner-to-corner in the SAME frame the
    // gradient's UV `(0,0)`..`(1,1)` anchors to. The ramp spanned the shape
    // before R1358 too (nothing here looked wrong — the migration is
    // pixel-identical); what changed is that the producer no longer has to
    // keep two bases in agreement by hand, because there is only one.
    let linear_rect = vec![
        PathCommand::MoveTo(pt((0.0, 0.0))),
        PathCommand::LineTo(pt((95.0, 0.0))),
        PathCommand::LineTo(pt((95.0, 52.0))),
        PathCommand::LineTo(pt((0.0, 52.0))),
        PathCommand::Close,
    ];
    let linear = path_node(
        "grad_linear",
        (45, 168),
        (95, 52),
        linear_rect,
        PathStyle::default().with_gradient(
            Gradient::horizontal()
                .with_stop(0.0, GRAD_A)
                .with_stop(0.5, GRAD_MID)
                .with_stop(1.0, GRAD_B),
        ),
    );

    // Radial: a diamond whose exact centre is the centre stop.
    let radial = path_node(
        "grad_radial",
        (267, 160),
        (76, 70),
        closed_polygon(&[(38.0, 0.0), (76.0, 35.0), (38.0, 70.0), (0.0, 35.0)]),
        PathStyle::default().with_gradient(
            Gradient::radial((0.5, 0.5), 0.5)
                .with_stop(0.0, GRAD_B)
                .with_stop(1.0, GRAD_A),
        ),
    );

    vec![linear, radial]
}

/// Build the RPC-drivable mode switch — a rounded chip tagged
/// `main_toggle` (the Toggle hit-test handle). Off sits on the inactive
/// container surface, On on the accent; Hover / Pressed lerp toward
/// on-surface at the M3 state-layer weights. Factored out of [`view`]
/// so the view-fn stays within the §6.3 pure-mapping line budget.
fn mode_switch(
    theme: &pinion_core::Theme,
    state: ToggleState,
    on: bool,
    on_surface: Color,
    surface: Color,
    accent: Color,
) -> Scene {
    let switch_base = if on {
        accent
    } else {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    };
    let switch_fill: Color = match state {
        ToggleState::Idle => switch_base,
        ToggleState::Hover => switch_base.lerp(on_surface, HOVER_OVERLAY_T),
        ToggleState::Pressed => switch_base.lerp(on_surface, PRESSED_OVERLAY_T),
        ToggleState::Disabled => switch_base.lerp(surface, DISABLED_OVERLAY_T),
    };
    let switch_fg = if on {
        theme.resolve(ColorRole::OnAccent)
    } else {
        on_surface
    };
    let switch_label = Scene::Text(TextNode::styled(
        if on { "Fill + stroke" } else { "Fill only" },
        Rect::default(),
        TextStyle::new()
            .with_size_px(SWITCH_FONT_PX)
            .with_fg(switch_fg),
    ));
    Scene::Container(
        ContainerNode::new(vec![switch_label])
            .with_tag("main_toggle")
            .with_aria_label("Diamond style")
            .with_style(BoxStyle::filled(switch_fill).with_corner_radius(18))
            .with_layout(
                LayoutStyle::new()
                    .with_focusable(true)
                    .with_absolute_position(110, 330)
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(140, 36)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `on` selects the demo-diamond style (Off = fill-only, On = fill +
/// stroke). `&Frame` is the §6.3 ZST hedge. The shell calls
/// `compute_layout` before paint, so the view need not resolve rects.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let accent = theme.resolve(ColorRole::Accent);

    let title = Scene::Text(
        TextNode::styled(
            "Vector paths",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(20, 16)),
    );

    // Filled triangle — fill-only, non-zero winding.
    let tri = path_node(
        "tri",
        (50, 80),
        (80, 80),
        closed_polygon(&TRI),
        PathStyle::filled(PATH_BLUE),
    );

    // Stroked chevron — stroke-only, round cap, straight segments.
    let chevron = path_node(
        "chevron",
        (160, 90),
        (100, 60),
        open_polyline(&CHEVRON),
        PathStyle::stroked(Stroke::new(STROKE_TEAL, STROKE_W).with_cap(StrokeCap::Round)),
    );

    // Cubic-Bezier arc — stroke-only, exercises CurveTo.
    let arc = path_node(
        "arc",
        (280, 80),
        (60, 60),
        arc_commands(),
        PathStyle::stroked(Stroke::new(STROKE_TEAL, STROKE_W).with_cap(StrokeCap::Round)),
    );

    // Demo diamond — Off = fill-only `PATH_BLUE`; On = fill
    // `DEMO_ON_FILL` + a `DEMO_ON_STROKE` outline. The PathStyle change
    // re-keys the §5.16 R682 paint-cache via `Hash for PathNode`.
    let demo_style = if on {
        PathStyle::filled(DEMO_ON_FILL)
            .with_stroke(Stroke::new(DEMO_ON_STROKE, DEMO_STROKE_W).with_cap(StrokeCap::Round))
    } else {
        PathStyle::filled(PATH_BLUE)
    };
    let demo_path = path_node(
        "demo_path",
        (110, 230),
        (100, 80),
        closed_polygon(&DIAMOND),
        demo_style,
    );

    let mode_chip = mode_switch(&theme, state, on, on_surface, surface, accent);

    let [grad_linear, grad_radial] = gradient_paths()
        .try_into()
        .expect("gradient_paths returns exactly two paths");

    let status = Scene::Text(
        TextNode::styled(
            format!(
                "{} | {}",
                state.as_name(),
                if on { "Fill + stroke" } else { "Fill only" }
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(20, 372)),
    );

    Scene::Container(
        ContainerNode::new(vec![
            title,
            tri,
            chevron,
            arc,
            demo_path,
            grad_linear,
            grad_radial,
            mode_chip,
            status,
        ])
        .with_style(BoxStyle::filled(surface))
        .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// `WidgetView` binding. The §5.38 Toggle is reused as the fill-only /
/// fill+stroke mode bit (see the module doc); the `#[widget]` attribute
/// derives the mechanical [`WidgetCore`] / [`WidgetA11y`] /
/// [`WidgetView`] trio. No `update` reducer — the Off/On bit is read
/// straight in [`view`].
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-path (R721 §5.16 vector-path substrate)",
    renderer = HelloPathRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ToggleExternal::new,
    role = Switch,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
        checked = bool_field(1),
    ),
    access_value = bool_field(1),
    apply_key = aria_activate,
    keybinding,
    event_name_derive,
)]
struct PathView;

impl PathView {
    /// Tuple-state introspect: SCXML state name via `query("state")` +
    /// the Off/On sidecar via `query("value")`. Defaults to
    /// `(Idle, false)` for a fresh External.
    fn read_state(scene: &Scene) -> (ToggleState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Ok(IntrospectValue::Text(name)) = intro.query("state") {
                    ToggleState::from_name_or_default(&name)
                } else {
                    ToggleState::Idle
                };
                let value = matches!(intro.query("value"), Ok(IntrospectValue::Bool(true)));
                return (state, value);
            }
        }
        (ToggleState::Idle, false)
    }

    /// R641 §5.16 inherent view shim — unpacks the tuple and forwards to
    /// the free [`view`].
    fn view(state: (ToggleState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    fn keybinding(key: &str) -> Option<ToggleEvent> {
        match key {
            "d" => Some(ToggleEvent::Disable),
            "e" => Some(ToggleEvent::Enable),
            _ => None,
        }
    }
}

fn main() {
    pinion_shell::run::<PathView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// Walk the scene for the first `Scene::Path` carrying `tag`.
    fn find_path<'a>(scene: &'a Scene, tag: &str) -> Option<&'a PathNode> {
        match scene {
            Scene::Path(p) if p.tag.as_deref() == Some(tag) => Some(p),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_path(ch, tag)),
            _ => None,
        }
    }

    fn rendered(state: ToggleState, on: bool) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, on, &Frame::new()))
    }

    #[test]
    fn triangle_is_a_closed_fill_only_polygon() {
        let scene = rendered(ToggleState::Idle, false);
        let tri = find_path(&scene, "tri").expect("triangle present");
        // MoveTo + 2 LineTo + Close.
        assert_eq!(tri.commands.len(), 4);
        assert!(matches!(tri.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(tri.commands[1], PathCommand::LineTo(_)));
        assert!(matches!(tri.commands[3], PathCommand::Close));
        assert_eq!(tri.style.fill, Some(PATH_BLUE));
        assert!(tri.style.stroke.is_none());
    }

    #[test]
    fn chevron_is_a_stroke_only_open_polyline() {
        let scene = rendered(ToggleState::Idle, false);
        let chevron = find_path(&scene, "chevron").expect("chevron present");
        // MoveTo + 2 LineTo, no Close.
        assert_eq!(chevron.commands.len(), 3);
        assert!(
            chevron
                .commands
                .iter()
                .all(|c| !matches!(c, PathCommand::Close))
        );
        assert!(chevron.style.fill.is_none());
        let stroke = chevron.style.stroke.expect("chevron has a stroke");
        assert_eq!(stroke.color, STROKE_TEAL);
        assert_eq!(stroke.width, STROKE_W);
        assert_eq!(stroke.cap, StrokeCap::Round);
    }

    #[test]
    fn arc_carries_a_cubic_curve_command() {
        let scene = rendered(ToggleState::Idle, false);
        let arc = find_path(&scene, "arc").expect("arc present");
        assert!(matches!(arc.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(arc.commands[1], PathCommand::CurveTo { .. }));
        assert!(arc.style.stroke.is_some());
    }

    #[test]
    fn diamond_is_fill_only_off_and_fill_plus_stroke_on() {
        let off = rendered(ToggleState::Idle, false);
        let off_d = find_path(&off, "demo_path").expect("diamond present");
        assert_eq!(off_d.style.fill, Some(PATH_BLUE));
        assert!(off_d.style.stroke.is_none());

        let on = rendered(ToggleState::Idle, true);
        let on_d = find_path(&on, "demo_path").expect("diamond present");
        assert_eq!(on_d.style.fill, Some(DEMO_ON_FILL));
        let stroke = on_d.style.stroke.expect("On adds a stroke");
        assert_eq!(stroke.color, DEMO_ON_STROKE);
    }

    #[test]
    fn diamond_style_change_re_keys_the_paint_hash() {
        // The §5.16 R682 paint-cache keys off `Scene::paint_hash`, which
        // folds `PathStyle` in via the manual `Hash for PathNode`. Off
        // vs On diamonds must therefore hash differently or the cache
        // would replay a stale fragment.
        let off = rendered(ToggleState::Idle, false);
        let on = rendered(ToggleState::Idle, true);
        assert_ne!(off.paint_hash(), on.paint_hash());
    }

    #[test]
    fn all_four_paths_are_present() {
        let scene = rendered(ToggleState::Idle, false);
        for tag in ["tri", "chevron", "arc", "demo_path"] {
            assert!(find_path(&scene, tag).is_some(), "missing path {tag}");
        }
    }

    #[test]
    fn grad_linear_is_a_three_stop_horizontal_gradient_fill() {
        use pinion_core::style::GradientKind;
        let scene = rendered(ToggleState::Idle, false);
        let g = find_path(&scene, "grad_linear").expect("grad_linear present");
        assert!(g.style.fill.is_none(), "gradient path has no solid fill");
        let gradient = g
            .style
            .gradient
            .as_ref()
            .expect("grad_linear has a gradient");
        assert!(matches!(gradient.kind, GradientKind::Linear { .. }));
        assert_eq!(gradient.stops.len(), 3);
        assert_eq!(gradient.stops[0].color, GRAD_A);
        assert_eq!(gradient.stops[1].color, GRAD_MID);
        assert_eq!(gradient.stops[2].color, GRAD_B);
    }

    #[test]
    fn grad_radial_centre_stop_is_grad_b() {
        use pinion_core::style::GradientKind;
        let scene = rendered(ToggleState::Idle, false);
        let g = find_path(&scene, "grad_radial").expect("grad_radial present");
        let gradient = g
            .style
            .gradient
            .as_ref()
            .expect("grad_radial has a gradient");
        assert!(matches!(gradient.kind, GradientKind::Radial { .. }));
        assert_eq!(gradient.stops.len(), 2);
        // The centre (offset 0) stop is the exact-pixel anchor.
        assert_eq!(gradient.stops[0].color, GRAD_B);
        assert_eq!(gradient.stops[1].color, GRAD_A);
    }

    #[test]
    fn gradient_fill_re_keys_the_paint_hash_vs_solid() {
        use std::hash::{Hash, Hasher};
        // A gradient path must hash differently from the same shape with
        // a solid fill, or the §5.16 R682 paint-cache would replay a
        // stale solid fragment. Exercises `Hash for PathStyle`.
        let g = PathStyle::default().with_gradient(
            Gradient::horizontal()
                .with_stop(0.0, GRAD_A)
                .with_stop(1.0, GRAD_B),
        );
        let solid = PathStyle::filled(GRAD_A);
        let h = |s: &PathStyle| {
            let mut hasher = std::hash::DefaultHasher::new();
            s.hash(&mut hasher);
            hasher.finish()
        };
        assert_ne!(h(&g), h(&solid));
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<PathView>(
            (ToggleState::Idle, false),
            &Frame::new(),
        );
    }

    #[test]
    fn r51_64_switch_reports_switch_role() {
        let nodes = <PathView as WidgetA11y>::access_node(&(ToggleState::Idle, true), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, "main_toggle");
    }
}
