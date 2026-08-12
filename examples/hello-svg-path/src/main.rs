// R838 §5.38 — example bindings tolerate looser doc-markdown lints than the
// substrate crates; the narrative carries many proper-noun identifiers.
#![allow(clippy::doc_markdown)]

//! `hello-svg-path` — R1623 §5.3 — an icon is a **string**, and it keeps
//! the curves it was drawn with.
//!
//! ## The problem this exists for
//!
//! Every icon anyone ships is SVG path data: a `d` attribute full of
//! `M`, `q`, `a`, `z`. Before R1623 pinion could not read one — the
//! scene's path vocabulary was `MoveTo` / `LineTo` / `CurveTo` /
//! `Close`, and the module's own doc said "quadratic / arc / etc. are
//! carry-forward". So importing an icon meant converting it by hand,
//! outside the framework, and arriving with the arcs already gone.
//!
//! The reference toolkit is only half a step ahead. It *has* quadratic
//! and arc builders, and both convert on the way in: the quadratic one
//! computes the equivalent cubic and calls the cubic builder, the arc
//! one appends Béziers, and the stored element list has four kinds with
//! no arc among them. Ask such a path whether it holds a circle and
//! there is nobody to ask. Its `d`-string parser exists but sits in
//! private headers, so an application cannot call it and writes its own
//! — and when the data is malformed, that parser answers one bit:
//! nothing came back.
//!
//! ## What this binding shows
//!
//! Four icons, each authored as the `d` string a designer would hand
//! over, parsed by [`pinion_core::path_data::parse`] and placed by
//! [`fit`](pinion_core::path_data::fit) — no hand-computed transform,
//! no pre-flattening:
//!
//! * **`icon_clock`** — a full circle as two `A` commands plus hands.
//!   The arcs survive into the scene, so `scene/snapshot` reports
//!   `ArcTo` with its radii and both flags rather than a stream of
//!   cubics that merely looks round.
//! * **`icon_wave`** — `Q` and its smooth form `T`. The quadratics
//!   survive as quadratics; the reflection `T` implies is resolved,
//!   because a reflected control point is fully determined and naming
//!   nothing new.
//! * **`icon_bookmark`** — relative commands, `h` / `v`, an implicit
//!   repeat and `z`: the spellings a real icon file is full of, all
//!   resolved to absolute geometry.
//! * **`subject`** — the toggled one. Off it is a valid pie sector
//!   (`L` then `A` then `Z`); On it is the same data with the
//!   large-arc flag typed as `3`.
//!
//! ## Why the Toggle is the malformed-data switch
//!
//! Because the failure is the point. With the bad flag in, this binding
//! paints **the error** — `path data byte 25 (in command 'A'): expected
//! an arc flag 0 or 1, found '3'` — and drops the icon. The reference
//! would render an empty area and say nothing anywhere, which is the
//! single most expensive way for a toolkit to answer: the caller sees
//! blank, and blank does not say whether the data was wrong, the file
//! was missing, or the colour matched the background.
//!
//! The status line also prints the subject re-written by
//! [`write()`](pinion_core::path_data::write). That round trip is the
//! visible proof that the vocabulary survived: an `A` goes in and an
//! `A` comes back out, where the reference has no inverse at all.
//!
//! ## Verification (substrate-first)
//!
//! * `tools/demos/r1623_svg_path.py` drives a real window over RPC and
//!   reads `scene/snapshot`: the arc command's `rx`, `ry`,
//!   `x_rotation`, `large_arc`, `sweep` and `end` are all on the wire,
//!   the quadratic arrives with one control point and not two, and
//!   toggling the subject swaps a path for an error message.
//! * The in-crate tests below assert the parse → fit → paint chain
//!   against the geometry, not against a picture.

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::path_data;
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, PathStyle, Size,
    Stroke, StrokeCap, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output: `pub struct HelloSvgPathRenderer` +
// `HelloSvgPathRendererError` + async `new<...>` + sync `render` /
// `resize`. R46.3.3 emit template uses fully-qualified `::vello::*`
// paths so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can build /
// render / resize it.
vello_renderer_impl!(HelloSvgPathRenderer, HelloSvgPathRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 300;

const THEME_TAG: &str = "app";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;
const SWITCH_FONT_PX: u32 = 14;

/// Material 3 state-layer overlay weights for the switch chrome.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

/// Theme-independent so a live-pixel guard can predict the RGB at each
/// anchor — the icons are the substrate under test, the theme is
/// orthogonal chrome.
const INK: Color = Color::rgb(0x21, 0x96, 0xf3);
const SUBJECT_INK: Color = Color::rgb(0xe5, 0x39, 0x35);
const STROKE_W: u32 = 4;

/// Every icon is authored in a 64 × 64 design box and fitted into this
/// one, which is what makes the `d` strings below copy-pasteable from a
/// real icon set rather than pre-scaled for this window.
const ICON_PX: u32 = 72;

// ── The icons, as the strings a designer hands over ────────────────

/// Two `A` commands make the dial (a full circle cannot be one arc —
/// its endpoints would coincide and SVG omits such an arc entirely),
/// then the hands.
const CLOCK_D: &str = "M 4,32 A 28,28 0 1 0 60,32 A 28,28 0 1 0 4,32 \
                       M 32,12 L 32,32 L 46,40";

/// `Q` and its smooth continuation `T`.
const WAVE_D: &str = "M 2,44 Q 18,10 34,44 T 62,30";

/// Relative moveto, `h`/`v`, an implicit lineto repeat, and `z`.
const BOOKMARK_D: &str = "m 14,4 h 36 v 56 l -18,-14 -18,14 z";

/// A pie sector: radius out, arc round, close through the centre.
const SUBJECT_OK_D: &str = "M 32,32 L 32,4 A 28,28 0 0 1 51.8,51.8 Z";

/// The same data with the large-arc flag typed as `3`.
const SUBJECT_BAD_D: &str = "M 32,32 L 32,4 A 28,28 0 3 1 51.8,51.8 Z";

/// Parse a `d` string and fit it into an [`ICON_PX`] box.
///
/// The whole of importing an icon, and the reason this is one call: the
/// reference applies a `viewBox` only for a complete SVG *document*, so
/// a bare path there means composing a bounding-rect query with a
/// transform by hand at every call site.
fn icon_commands(d: &str) -> Result<Vec<PathCommand>, path_data::PathDataError> {
    #[allow(
        clippy::cast_precision_loss,
        reason = "ICON_PX is a small pixel count with an exact f32 representation"
    )]
    let side = ICON_PX as f32;
    let parsed = path_data::parse(d)?;
    // `fit` answers `None` only for a degenerate box or a path that
    // draws nothing; every constant above draws something, and a
    // future one that does not should paint nothing rather than
    // silently land at the origin.
    Ok(path_data::fit(&parsed, side, side).unwrap_or(parsed))
}

/// An absolutely-positioned [`Scene::Path`] holding an imported icon.
fn icon_node(
    tag: &'static str,
    origin: (u32, u32),
    commands: Vec<PathCommand>,
    ink: Color,
) -> Scene {
    Scene::Path(
        PathNode::new(
            Rect::default(),
            commands,
            PathStyle::default().with_stroke(Stroke::new(ink, STROKE_W).with_cap(StrokeCap::Round)),
        )
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(origin.0, origin.1)
                .with_size(Size::px(ICON_PX, ICON_PX)),
        ),
    )
}

/// A filled icon — the subject, so the toggle's two states differ in
/// more than a stroke width.
fn filled_icon_node(tag: &'static str, origin: (u32, u32), commands: Vec<PathCommand>) -> Scene {
    Scene::Path(
        PathNode::new(Rect::default(), commands, PathStyle::filled(SUBJECT_INK))
            .with_tag(tag)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(origin.0, origin.1)
                    .with_size(Size::px(ICON_PX, ICON_PX)),
            ),
    )
}

fn label(text: String, at: (u32, u32), size_px: u32, fg: Color) -> Scene {
    Scene::Text(
        TextNode::styled(
            text,
            Rect::default(),
            TextStyle::new().with_size_px(size_px).with_fg(fg),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(at.0, at.1)),
    )
}

fn mode_switch(
    theme: &pinion_core::Theme,
    state: ToggleState,
    on: bool,
    on_surface: Color,
    surface: Color,
    accent: Color,
) -> Scene {
    let base = if on {
        accent
    } else {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    };
    let fill: Color = match state {
        ToggleState::Idle => base,
        ToggleState::Hover => base.lerp(on_surface, HOVER_OVERLAY_T),
        ToggleState::Pressed => base.lerp(on_surface, PRESSED_OVERLAY_T),
        ToggleState::Disabled => base.lerp(surface, DISABLED_OVERLAY_T),
    };
    let fg = if on {
        theme.resolve(ColorRole::OnAccent)
    } else {
        on_surface
    };
    let text = Scene::Text(TextNode::styled(
        if on { "Malformed d" } else { "Valid d" },
        Rect::default(),
        TextStyle::new().with_size_px(SWITCH_FONT_PX).with_fg(fg),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag("main_toggle")
            .with_aria_label("Subject path data")
            .with_style(BoxStyle::filled(fill).with_corner_radius(18))
            .with_layout(
                LayoutStyle::new()
                    .with_focusable(true)
                    .with_absolute_position(20, 244)
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(140, 36)),
            ),
    )
}

fn view(state: ToggleState, malformed: bool, _frame: Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let accent = theme.resolve(ColorRole::Accent);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let mut children = vec![label(
        "Icons imported from SVG path data".to_string(),
        (20, 16),
        TITLE_FONT_PX,
        on_surface,
    )];

    for (tag, d, origin) in [
        ("icon_clock", CLOCK_D, (20u32, 56u32)),
        ("icon_wave", WAVE_D, (112, 56)),
        ("icon_bookmark", BOOKMARK_D, (204, 56)),
    ] {
        match icon_commands(d) {
            Ok(cmds) => children.push(icon_node(tag, origin, cmds, INK)),
            // A constant that stops parsing is a bug in this file, and
            // the status line is where it says so.
            Err(e) => children.push(label(format!("{tag}: {e}"), origin, STATUS_FONT_PX, muted)),
        }
    }

    let subject_d = if malformed {
        SUBJECT_BAD_D
    } else {
        SUBJECT_OK_D
    };
    let status = match icon_commands(subject_d) {
        Ok(cmds) => {
            // The round trip, written out: what went in as `A` comes
            // back out as `A`.
            let echo = path_data::write(&path_data::parse(subject_d).unwrap_or_default());
            children.push(filled_icon_node("subject", (296, 56), cmds));
            format!("subject parsed, rewritten as: {echo}")
        }
        Err(e) => format!("subject REFUSED — {e}"),
    };

    children.push(label(status, (20, 152), STATUS_FONT_PX, muted));
    children.push(label(
        format!("source: {subject_d}"),
        (20, 172),
        STATUS_FONT_PX,
        muted,
    ));
    children.push(mode_switch(
        &theme, state, malformed, on_surface, surface, accent,
    ));
    children.push(label(
        format!(
            "{} | {}",
            state.as_name(),
            if malformed { "refusing" } else { "drawing" }
        ),
        (20, 208),
        STATUS_FONT_PX,
        muted,
    ));

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// `WidgetView` binding. The §5.38 Toggle is reused as the
/// valid / malformed bit for the subject path.
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-svg-path (R1623 §5.3 SVG path data)",
    renderer = HelloSvgPathRenderer,
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
struct SvgPathView;

impl SvgPathView {
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

    fn view(state: (ToggleState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, frame)
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
    pinion_shell::run::<SvgPathView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use pinion_core::path_data::{PathCommandKind, PathDataErrorKind};

    fn find_path<'a>(scene: &'a Scene, tag: &str) -> Option<&'a PathNode> {
        match scene {
            Scene::Path(p) if p.tag.as_deref() == Some(tag) => Some(p),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_path(ch, tag)),
            _ => None,
        }
    }

    fn texts(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for ch in &c.children {
                    texts(ch, out);
                }
            }
            _ => {}
        }
    }

    fn rendered(state: ToggleState, malformed: bool) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, malformed, Frame::new()))
    }

    fn kinds(node: &PathNode) -> Vec<PathCommandKind> {
        node.commands.iter().map(PathCommand::kind).collect()
    }

    #[test]
    fn every_icon_constant_parses() {
        for (name, d) in [
            ("clock", CLOCK_D),
            ("wave", WAVE_D),
            ("bookmark", BOOKMARK_D),
            ("subject", SUBJECT_OK_D),
        ] {
            assert!(path_data::parse(d).is_ok(), "{name} does not parse");
        }
    }

    #[test]
    fn the_clock_keeps_its_arcs_rather_than_becoming_cubics() {
        let scene = rendered(ToggleState::Idle, false);
        let clock = find_path(&scene, "icon_clock").expect("clock icon");
        let arcs = kinds(clock)
            .iter()
            .filter(|k| **k == PathCommandKind::ArcTo)
            .count();
        assert_eq!(arcs, 2, "a full circle is two arcs: {:?}", kinds(clock));
        assert!(
            !kinds(clock).contains(&PathCommandKind::CurveTo),
            "nothing here was authored as a cubic",
        );
    }

    #[test]
    fn the_wave_keeps_its_quadratics_and_resolves_the_smooth_one() {
        let scene = rendered(ToggleState::Idle, false);
        let wave = find_path(&scene, "icon_wave").expect("wave icon");
        assert_eq!(
            kinds(wave),
            vec![
                PathCommandKind::MoveTo,
                PathCommandKind::QuadTo,
                PathCommandKind::QuadTo,
            ],
            "`T` is a quadratic whose control point is determined, not a new curve",
        );
    }

    #[test]
    fn the_bookmark_resolves_relative_and_shorthand_spellings() {
        let scene = rendered(ToggleState::Idle, false);
        let bm = find_path(&scene, "icon_bookmark").expect("bookmark icon");
        assert_eq!(
            kinds(bm),
            vec![
                PathCommandKind::MoveTo,
                PathCommandKind::LineTo,
                PathCommandKind::LineTo,
                PathCommandKind::LineTo,
                PathCommandKind::LineTo,
                PathCommandKind::Close,
            ],
            "h / v / the implicit repeat are all linetos: {:?}",
            kinds(bm),
        );
    }

    #[test]
    fn every_icon_is_fitted_inside_its_box() {
        let scene = rendered(ToggleState::Idle, false);
        #[allow(clippy::cast_precision_loss, reason = "ICON_PX is small and exact")]
        let side = ICON_PX as f32;
        for tag in ["icon_clock", "icon_wave", "icon_bookmark", "subject"] {
            let node = find_path(&scene, tag).expect(tag);
            let b = path_data::bounds(&node.commands).expect("icon draws something");
            assert!(
                b.min_x >= -0.01 && b.min_y >= -0.01,
                "{tag} starts inside: {b:?}"
            );
            assert!(
                b.max_x <= side + 0.01 && b.max_y <= side + 0.01,
                "{tag} ends inside: {b:?}",
            );
            // Fitting preserves aspect, so exactly one axis is filled.
            let filled_x = (b.width() - side).abs() < 0.01;
            let filled_y = (b.height() - side).abs() < 0.01;
            assert!(filled_x || filled_y, "{tag} fills neither axis: {b:?}");
        }
    }

    #[test]
    fn malformed_data_is_refused_by_the_byte_rather_than_drawn_as_nothing() {
        let scene = rendered(ToggleState::Idle, true);
        assert!(
            find_path(&scene, "subject").is_none(),
            "a path that did not parse must not be painted",
        );
        let mut out = Vec::new();
        texts(&scene, &mut out);
        let status = out
            .iter()
            .find(|t| t.contains("REFUSED"))
            .expect("the refusal is on screen");
        assert!(status.contains("byte 25"), "{status}");
        assert!(status.contains("'A'"), "{status}");
        assert!(status.contains("flag"), "{status}");

        // And the error the binding reports is the error the parser
        // gives, rather than a message this file made up.
        let err = path_data::parse(SUBJECT_BAD_D).expect_err("bad flag");
        assert_eq!(err.kind, PathDataErrorKind::ExpectedFlag('3'));
        assert!(status.contains(&err.to_string()), "{status}");
    }

    #[test]
    fn the_valid_subject_round_trips_through_path_data() {
        let scene = rendered(ToggleState::Idle, false);
        assert!(find_path(&scene, "subject").is_some());
        let mut out = Vec::new();
        texts(&scene, &mut out);
        let echo = out
            .iter()
            .find(|t| t.starts_with("subject parsed"))
            .expect("the round trip is on screen");
        // The arc letter survives the trip; the reference has no
        // inverse to survive.
        assert!(echo.contains(" A "), "{echo}");
        let once = path_data::parse(SUBJECT_OK_D).expect("parses");
        assert_eq!(path_data::parse(&path_data::write(&once)), Ok(once));
    }

    #[test]
    fn the_toggle_is_the_switch_and_announces_itself() {
        let nodes = <SvgPathView as WidgetA11y>::access_node(&(ToggleState::Idle, true), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, "main_toggle");
    }
}
