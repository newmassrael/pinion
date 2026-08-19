// R1416 §5.35 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-raw-pointer` — R1416 §5.35 §5.15 — a **raw multi-button pointer
//! sink**, the forcing consumer of the new
//! [`External::wants_raw_pointer_buttons`] seam.
//!
//! ## What this demonstrates
//!
//! The default pipeline routes only the LEFT button to a widget (as the
//! `send`-wire `PointerDown` / `PointerUp`), and its right / middle presses
//! drive GUI *default actions* — right opens the context menu, middle pastes
//! the PRIMARY selection, middle-drag pans. A widget that IS the pointer
//! authority for its region — a terminal pane forwarding xterm mouse reports,
//! a game viewport, a remote-desktop surface — needs the raw button EDGES, not
//! those GUI interpretations.
//!
//! [`RawPointerSink`] opts into [`External::wants_raw_pointer_buttons`], so the
//! router delivers EVERY mouse button (left / middle / right) on BOTH the press
//! and release edge, each carrying the modifiers held at that edge, with the
//! button identified — through the typed
//! [`External::raw_pointer_button`] method. The three gaps the sprag PR-72
//! report named are each closed here:
//!
//!   * **right RELEASE** — previously the shell had no right-release arm at all
//!     (`_ => {}` swallowed it), so a right press could report but never pair
//!     its release (an SGR app would see a stuck button). The sink receives
//!     `right:up`.
//!   * **press modifiers** — the legacy `PointerDown` send wire routed the
//!     press through a zero-modifier dispatch, so a Shift-press reported no
//!     Shift. The raw stream carries modifiers on the DOWN edge too
//!     (`right:down:s`).
//!   * **button identity** — the legacy wire named only `PointerDown` /
//!     `PointerUp` with no button. Each raw edge names its button.
//!
//! The suppression is scoped to THIS widget (the non-capture invariant): every
//! other widget keeps left = focus / select, middle = PRIMARY paste, right =
//! context menu. Only the widget that owns the raw stream trades them — the W3C
//! model, where a listener that handles `mousedown` opts out of the default.
//! Position is NOT on the button event; the sink tracks it via
//! [`External::wants_hover_move`] (a bare hover forwards `pointer_move`) and
//! STAMPS the live fraction onto each report — exactly how a pane oracle
//! correlates a button edge with the terminal cell under the cursor.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! The sink exposes the raw stream as introspectable state a snapshot cannot
//! phrase as one field: `report_count`, `last` (`button:edge:mods`),
//! `last_button` / `last_edge` / `last_mods`, `last_x` / `last_y` (the position
//! stamped at the last edge), `x_frac` / `y_frac` (the live hover position),
//! `pressure` (R1423 — the live W3C `PointerEvent.pressure`, driving the ink
//! mark's diameter), `tilt_x` / `tilt_y` (R1429 — the live W3C
//! `PointerEvent.tiltX/tiltY`, leaning the pen-tip marker off the cursor), and
//! `log` (the full `;`-joined sequence). Every button edge is driven no-pixel
//! via the `scene/pointer_button` RPC method — the single-edge peer the
//! press-pair `scene/click` never expressed — the pressure via
//! `scene/pointer_pressure` (R1423), and the tilt via `scene/pointer_tilt`
//! (R1429). See `tools/demos/r1416_raw_pointer.py`,
//! `tools/demos/r1423_pressure.py`, and `tools/demos/r1429_tilt.py`.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaField,
    ThreadOwnership,
};
use pinion_core::input::PointerReading;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{Border, BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{
    Frame, Modifiers, PointerButton, PointerButtons, PointerEdge, PointerKind, RawPointerButton,
    Scene, WidgetCore,
};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRawPointerRenderer, HelloRawPointerRendererError);

const WIN_W: u32 = 720;
const WIN_H: u32 = 440;
const THEME_TAG: &str = "app";

/// The pane's paint tag **and** the primary [`RawPointerSink`]'s registration
/// tag — addressed over RPC as `/external/<field>`. A transparent, pointer-
/// opaque surface over the pane carries it, so a button anywhere over the pane
/// routes to the sink.
const PANE_TAG: &str = "pane";

/// The human-readable report line at the window's foot.
const READOUT_TAG: &str = "raw.readout";

/// R1423 — the pressure-reactive ink mark's paint tag. A snapshot reads its rect
/// to confirm the mark grows with pressure (W3C `PointerEvent.pressure`).
const INK_TAG: &str = "ink.dot";

/// R1423 — the ink mark's diameter at zero-and-a-hair pressure (px) and the span
/// it grows across to full force, so `diameter = MIN + pressure * RANGE`.
const DOT_MIN_PX: f32 = 6.0;
const DOT_RANGE_PX: f32 = 44.0;

/// R1429 — the tilt indicator's paint tag. A snapshot reads its rect to confirm
/// the pen-tip marker leans off the cursor in the pen's tilt direction.
const TIP_TAG: &str = "tilt.tip";

/// R1429 — the tilt marker's size (px) and the max pixel offset from the cursor
/// at full ±90° lean, so `offset = (tilt / 90) * TILT_SPAN_PX` px along each axis
/// (positive `tilt_x` leans right, positive `tilt_y` leans down — the W3C sign).
const TIP_SIZE_PX: u32 = 10;
const TILT_SPAN_PX: f32 = 40.0;

/// R1430 — the orientation dot ORBITS the pen tip at the twist angle (W3C
/// `PointerEvent.twist`, clockwise from straight-up), so barrel rotation reads on
/// screen: `centre = tip + ORBIT_RADIUS_PX * (sin twist, -cos twist)`.
const ORBIT_TAG: &str = "twist.orbit";
const ORBIT_SIZE_PX: u32 = 6;
const ORBIT_RADIUS_PX: f32 = 22.0;

/// R1430 — the finger-wheel bar: a horizontal fill whose width tracks the
/// tangential pressure (W3C `PointerEvent.tangentialPressure`),
/// `width = (tangential + 1) / 2 * TANG_BAR_W`, so the wheel reads on screen.
const TANG_TAG: &str = "tang.bar";
const TANG_BAR_X: u32 = 18;
const TANG_BAR_Y: u32 = WIN_H - 44;
const TANG_BAR_H: u32 = 6;
const TANG_BAR_W: f32 = 120.0;

/// R1430 — the pen-tip marker SHRINKS as the pen lifts (the toolkit `z()`): full
/// size at contact, down to `TIP_MIN_SCALE` at `HEIGHT_FULL_PX` and beyond.
const HEIGHT_FULL_PX: f32 = 30.0;
const TIP_MIN_SCALE: f32 = 0.35;

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 13;

/// Window-absolute pane region. The transparent sink surface covers exactly
/// this rect, so a pointer's `x_rel` / `y_rel` fraction `0.0..=1.0` is the
/// position the sink stamps onto each report.
const PANE_RECT: Rect = Rect::new(16, 48, WIN_W - 32, WIN_H - 104);

/// One raw button edge the sink recorded, with the live cursor fraction stamped
/// at the edge (the position a pane oracle correlates to a terminal cell).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ButtonReport {
    button: PointerButton,
    edge: PointerEdge,
    modifiers: Modifiers,
    /// R1418 — the full set of buttons held after this edge (the toolkit `buttons()`).
    buttons: PointerButtons,
    /// R1422 — the consecutive-click ordinal (the toolkit `MouseButtonDblClick` = 2), the
    /// router-synthesised double-click count carried on the raw edge.
    click_count: u8,
    x_frac: Option<f32>,
    y_frac: Option<f32>,
}

impl ButtonReport {
    /// The compact `"<button>:<edge>:<mods>"` label — the same shape the router
    /// unit tests assert, so the wire and the tests read one vocabulary. The
    /// R1422 click-count rides its own `last_clicks` field (like R1418's
    /// `last_buttons`), so the label stays the stable three-segment identity.
    fn label(&self) -> String {
        format!(
            "{}:{}:{}",
            self.button.as_wire_name(),
            self.edge.as_wire_name(),
            self.modifiers.as_wire_token(),
        )
    }
}

/// The Copy summary the view paints — the last report + the running count + the
/// live hover position (`WidgetCore::State` is `Copy`, so the full `Vec` log
/// lives in the external; the view needs only this digest).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct SinkState {
    count: usize,
    last: Option<ButtonReport>,
    x_frac: Option<f32>,
    y_frac: Option<f32>,
    /// R1423 — the live pointer pressure (W3C `PointerEvent.pressure`), `0.0..=1.0`.
    pressure: f32,
    /// R1429 — the live pointer tilt (W3C `PointerEvent.tiltX/tiltY`), in degrees,
    /// each axis `-90.0..=90.0`. Leans the pen-tip marker off the cursor.
    tilt_x: f32,
    tilt_y: f32,
    /// R1430 — the live pointer twist (W3C `PointerEvent.twist`), degrees
    /// `0.0..=360.0`; orbits the orientation dot around the pen tip.
    twist: f32,
    /// R1430 — the live tangential pressure (W3C `PointerEvent.tangentialPressure`),
    /// `-1.0..=1.0`; fills the finger-wheel bar.
    tangential: f32,
    /// R1430 — the live hover height (the toolkit `z()`), `>= 0.0`; shrinks
    /// the pen-tip marker as the pen lifts.
    height: f32,
    /// R1431 — the producing device (W3C `PointerEvent.pointerType`); colours the
    /// pen-tip marker (mouse / pen / eraser / touch).
    kind: PointerKind,
}

/// The idle prompt / live report line — the SSOT both the status text and the
/// a11y value read.
fn readout_text(state: &SinkState) -> String {
    match state.last {
        None => "press any mouse button over the pane (left / middle / right)".to_owned(),
        Some(r) => {
            let pos = match (r.x_frac, r.y_frac) {
                (Some(x), Some(y)) => format!(" @ ({x:.2}, {y:.2})"),
                _ => String::new(),
            };
            // R1422 — surface a synthesised double-click (the toolkit `MouseButtonDblClick`) as
            // a "×N" badge so the double is visible in the readout, not only
            // in the introspect field.
            let clicks = if r.click_count >= 2 {
                format!(" ×{}", r.click_count)
            } else {
                String::new()
            };
            // R1423 — surface the live pointer pressure (W3C
            // `PointerEvent.pressure`) so a pen / driven force reads as data.
            let force = if state.pressure > 0.0 {
                format!(" · pressure {:.2}", state.pressure)
            } else {
                String::new()
            };
            // R1429 — surface the live pointer tilt (W3C `PointerEvent.tiltX/tiltY`)
            // so a pen's lean reads as data. Present only when the pen is off the
            // perpendicular (a mouse / upright pen reports no tilt).
            let lean = if state.tilt_x.abs() > 0.0 || state.tilt_y.abs() > 0.0 {
                format!(
                    " · tilt ({:.0}\u{b0}, {:.0}\u{b0})",
                    state.tilt_x, state.tilt_y
                )
            } else {
                String::new()
            };
            // R1430 — surface the remaining the toolkit tablet event scalar
            // axes (twist / tangential / height), each only when off its
            // neutral rest.
            let barrel = if state.twist.abs() > 0.0 {
                format!(" · twist {:.0}\u{b0}", state.twist)
            } else {
                String::new()
            };
            let wheel = if state.tangential.abs() > 0.0 {
                format!(" · tang {:+.2}", state.tangential)
            } else {
                String::new()
            };
            let lift = if state.height > 0.0 {
                format!(" · z {:.1}", state.height)
            } else {
                String::new()
            };
            // R1431 — name the producing device (W3C `pointerType`) when it is not
            // the default mouse, so a stylus / eraser reads as data.
            let device = if state.kind == PointerKind::Mouse {
                String::new()
            } else {
                format!(" · {}", state.kind.as_wire_name())
            };
            format!(
                "#{} {}{}{}{}{}{}{}{}{}",
                state.count,
                r.label(),
                clicks,
                pos,
                force,
                lean,
                barrel,
                wheel,
                lift,
                device
            )
        }
    }
}

/// R1431 — the pen-tip marker colour for the producing device (W3C
/// `pointerType`): the accent for a pen, the error tone for an eraser (the DCC
/// "flip to erase" signal), a muted tone for a mouse, and the on-surface
/// foreground for touch — so the device reads at a glance, not only in the
/// readout.
fn tip_color(kind: PointerKind, theme: &pinion_core::theme::Theme) -> pinion_core::style::Color {
    theme.resolve(match kind {
        PointerKind::Mouse => ColorRole::OnSurface,
        PointerKind::Pen => ColorRole::Accent,
        PointerKind::Eraser => ColorRole::Error,
        PointerKind::Touch => ColorRole::OnSurfaceMuted,
    })
}

/// view-fn (§6.3): pure sync mapping of the sink digest to a scene.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the WidgetCore::view trait hands the frame by reference; the signature mirrors it"
)]
fn view(state: SinkState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);
    let pane_fill = theme.resolve(ColorRole::SurfaceContainerLow);
    let outline = theme.resolve(ColorRole::Outline);

    let title = Scene::Text(
        TextNode::styled(
            "Raw pointer sink — every button, both edges, with modifiers",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(18, 16)),
    );

    // The visible pane body, behind the transparent sink surface.
    let pane_body = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(pane_fill).with_border(Border::new(outline, 1)),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(PANE_RECT.x, PANE_RECT.y)
                .with_size(Size::px(PANE_RECT.w, PANE_RECT.h)),
        ),
    );

    // Transparent, pointer-opaque capture surface over the pane — the `pane`
    // primary tag the sink registers under. On top so a button anywhere over
    // the pane resolves to it; transparent so the body shows through. R1417
    // capture_surface lift.
    let pane_surface = capture_surface(PANE_TAG, PANE_RECT, false);

    let status = Scene::Text(
        TextNode::styled(
            readout_text(&state),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(on_surface_muted),
        )
        .with_tag(READOUT_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(18, WIN_H - 26)),
    );

    // R1423 — the pressure-reactive ink mark: a filled dot at the live cursor
    // whose diameter scales with the pointer pressure (W3C `PointerEvent.pressure`)
    // — the DCC ink-brush vignette. Present only when a force is applied over a
    // known position, so a plain hover (pressure 0) leaves no mark.
    let ink_dot = ink_dot_scene(&state, theme.resolve(ColorRole::Accent));

    // R1429 — the pen-tip marker: leans off the live cursor in the tilt direction
    // (W3C `PointerEvent.tiltX/tiltY`), shrinks with hover height (R1430), and is
    // coloured by the producing device (R1431). Present on any hover over the pane.
    let tilt_tip = tilt_tip_scene(&state, tip_color(state.kind, &theme));
    // R1430 — the twist orientation dot orbits the tip at the barrel angle.
    let twist_orbit = twist_orbit_scene(&state, theme.resolve(ColorRole::Accent));
    // R1430 — the finger-wheel bar tracks the tangential pressure (always shown).
    let tang_bar = tang_bar_scene(&state, theme.resolve(ColorRole::Accent));

    let mut children = vec![pane_body];
    if let Some(dot) = ink_dot {
        children.push(dot);
    }
    // The tip rides ON TOP of the ink mark so the lean stays visible under force.
    if let Some(tip) = tilt_tip {
        children.push(tip);
    }
    if let Some(orbit) = twist_orbit {
        children.push(orbit);
    }
    children.extend([tang_bar, pane_surface, title, status]);

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// R1423 — build the pressure-reactive ink mark, or `None` when there is no force
/// or no known position. A filled, fully-rounded dot centred on the live cursor;
/// `diameter = DOT_MIN_PX + pressure * DOT_RANGE_PX`, so the mark visibly grows
/// with force.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "the diameter and cursor pixel are small non-negative values; the fractional part is not meaningful for a paint rect"
)]
fn ink_dot_scene(state: &SinkState, ink: pinion_core::style::Color) -> Option<Scene> {
    let (fx, fy) = match (state.x_frac, state.y_frac) {
        (Some(fx), Some(fy)) if state.pressure > 0.0 => (fx.clamp(0.0, 1.0), fy.clamp(0.0, 1.0)),
        _ => return None,
    };
    let size = (DOT_MIN_PX + state.pressure.clamp(0.0, 1.0) * DOT_RANGE_PX) as u32;
    let half = size / 2;
    let cx = PANE_RECT.x + (fx * PANE_RECT.w as f32) as u32;
    let cy = PANE_RECT.y + (fy * PANE_RECT.h as f32) as u32;
    Some(Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(ink).with_corner_radius(half),
        )
        .with_tag(INK_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(cx.saturating_sub(half), cy.saturating_sub(half))
                .with_size(Size::px(size, size)),
        ),
    ))
}

/// R1429/R1430 — the pen-tip marker's CENTRE in window pixels, or `None` when
/// there is no known position. The cursor offset in the pen's lean direction:
/// `offset = (tilt / 90) * TILT_SPAN_PX` px along each axis (positive `tilt_x`
/// leans right, positive `tilt_y` down — the W3C sign). The SSOT the tip marker
/// and the R1430 twist orbit both build from, so they cannot drift.
#[allow(
    clippy::cast_precision_loss,
    reason = "the pane rect is a few-hundred-pixel extent; f32 carries it exactly"
)]
fn tip_center(state: &SinkState) -> Option<(f32, f32)> {
    let (fx, fy) = match (state.x_frac, state.y_frac) {
        (Some(fx), Some(fy)) => (fx.clamp(0.0, 1.0), fy.clamp(0.0, 1.0)),
        _ => return None,
    };
    let cx = PANE_RECT.x as f32 + fx * PANE_RECT.w as f32;
    let cy = PANE_RECT.y as f32 + fy * PANE_RECT.h as f32;
    let dx = (state.tilt_x.clamp(-90.0, 90.0) / 90.0) * TILT_SPAN_PX;
    let dy = (state.tilt_y.clamp(-90.0, 90.0) / 90.0) * TILT_SPAN_PX;
    Some((cx + dx, cy + dy))
}

/// A filled, fully-rounded dot of `size` px centred on `(cx, cy)` in window
/// pixels, tagged `tag` — the shared builder for the R1429 tip marker and the
/// R1430 twist orbit dot.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the centre and size are small non-negative pixels; the fraction is not meaningful for a paint rect"
)]
fn dot_scene(
    tag: &'static str,
    cx: f32,
    cy: f32,
    size: u32,
    color: pinion_core::style::Color,
) -> Scene {
    let half = size / 2;
    let x = (cx.max(0.0) as u32).saturating_sub(half);
    let y = (cy.max(0.0) as u32).saturating_sub(half);
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(color).with_corner_radius(half),
        )
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(x, y)
                .with_size(Size::px(size, size)),
        ),
    )
}

/// R1429/R1430 — build the tilt indicator, or `None` with no known position. The
/// marker sits at [`tip_center`] and SHRINKS as the pen lifts (the toolkit `z()`): full
/// size in contact, down to `TIP_MIN_SCALE` at `HEIGHT_FULL_PX`. Present on any hover over the pane,
/// independent of pressure.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "the tip size is a small non-negative pixel value"
)]
fn tilt_tip_scene(state: &SinkState, color: pinion_core::style::Color) -> Option<Scene> {
    let (cx, cy) = tip_center(state)?;
    let lift = (state.height.max(0.0) / HEIGHT_FULL_PX).min(1.0);
    let scale = 1.0 - lift * (1.0 - TIP_MIN_SCALE);
    let size = ((TIP_SIZE_PX as f32) * scale).round().max(3.0) as u32;
    Some(dot_scene(TIP_TAG, cx, cy, size, color))
}

/// R1430 — build the twist orientation dot, or `None` with no known position. It
/// ORBITS the pen tip at the barrel angle (W3C `PointerEvent.twist`), clockwise
/// from straight-up: `centre = tip + ORBIT_RADIUS_PX * (sin twist, -cos twist)`.
/// So twist 0 = above the tip, 90 = right, 180 = below, 270 = left.
fn twist_orbit_scene(state: &SinkState, color: pinion_core::style::Color) -> Option<Scene> {
    let (tx, ty) = tip_center(state)?;
    let rad = state.twist.rem_euclid(360.0).to_radians();
    let ox = tx + ORBIT_RADIUS_PX * rad.sin();
    let oy = ty - ORBIT_RADIUS_PX * rad.cos();
    Some(dot_scene(ORBIT_TAG, ox, oy, ORBIT_SIZE_PX, color))
}

/// R1430 — build the finger-wheel bar: a horizontal fill whose width tracks the
/// tangential pressure (W3C `PointerEvent.tangentialPressure`),
/// `width = (tangential + 1) / 2 * TANG_BAR_W`. Always present (the wheel has a
/// neutral rest at 0, a half-full bar), so it reads as a live gauge.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the bar width is a small non-negative pixel value"
)]
fn tang_bar_scene(state: &SinkState, color: pinion_core::style::Color) -> Scene {
    let frac = state
        .tangential
        .clamp(-1.0, 1.0)
        .midpoint(1.0)
        .clamp(0.0, 1.0);
    let width = (TANG_BAR_W * frac).round().max(1.0) as u32;
    Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(color))
            .with_tag(TANG_TAG)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(TANG_BAR_X, TANG_BAR_Y)
                    .with_size(Size::px(width, TANG_BAR_H)),
            ),
    )
}

/// Read the sink digest from the primary [`RawPointerSink`] in the state scene.
fn read_sink(scene: &Scene) -> SinkState {
    let Some(intro) = scene
        .find_external_with_tag(PANE_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return SinkState::default();
    };
    let count = match intro.query("report_count") {
        Ok(IntrospectValue::Int(n)) => usize::try_from(n).unwrap_or(0),
        _ => 0,
    };
    let last = read_last_report(intro);
    SinkState {
        count,
        last,
        x_frac: query_frac(intro, "x_frac"),
        y_frac: query_frac(intro, "y_frac"),
        pressure: query_frac(intro, "pressure").unwrap_or(0.0),
        tilt_x: query_frac(intro, "tilt_x").unwrap_or(0.0),
        tilt_y: query_frac(intro, "tilt_y").unwrap_or(0.0),
        twist: query_frac(intro, "twist").unwrap_or(0.0),
        tangential: query_frac(intro, "tangential").unwrap_or(0.0),
        height: query_frac(intro, "height").unwrap_or(0.0),
        kind: match intro.query("pointer_type") {
            Ok(IntrospectValue::Text(s)) => {
                PointerKind::from_wire_name(&s).unwrap_or(PointerKind::Mouse)
            }
            _ => PointerKind::Mouse,
        },
    }
}

/// Reassemble the last [`ButtonReport`] from the external's scalar query
/// surface — the same fields the AI-first `scene/query` client reads.
fn read_last_report(intro: &dyn ExternalIntrospect) -> Option<ButtonReport> {
    let button = match intro.query("last_button") {
        Ok(IntrospectValue::Text(s)) => PointerButton::from_wire_name(&s)?,
        _ => return None,
    };
    let edge = match intro.query("last_edge") {
        Ok(IntrospectValue::Text(s)) => PointerEdge::from_wire_name(&s)?,
        _ => return None,
    };
    let modifiers = match intro.query("last_mods") {
        Ok(IntrospectValue::Text(s)) => Modifiers::from_wire_token(&s).unwrap_or_default(),
        _ => Modifiers::empty(),
    };
    let buttons = match intro.query("last_buttons") {
        Ok(IntrospectValue::Text(s)) => PointerButtons::from_wire_token(&s).unwrap_or_default(),
        _ => PointerButtons::empty(),
    };
    let click_count = match intro.query("last_clicks") {
        Ok(IntrospectValue::Int(n)) => u8::try_from(n).unwrap_or(1),
        _ => 1,
    };
    Some(ButtonReport {
        button,
        edge,
        modifiers,
        buttons,
        click_count,
        x_frac: query_frac(intro, "last_x"),
        y_frac: query_frac(intro, "last_y"),
    })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "an inspect fraction 0.0..=1.0 loses no meaningful precision as f32"
)]
fn query_frac(intro: &dyn ExternalIntrospect, path: &str) -> Option<f32> {
    match intro.query(path) {
        Ok(IntrospectValue::Float(f)) => Some(f as f32),
        _ => None,
    }
}

// --- The raw pointer sink (primary) ----------------------------------------

/// The raw multi-button pointer authority. Opts into
/// [`External::wants_raw_pointer_buttons`] (the raw button edges) and
/// [`External::wants_hover_move`] (the live position it stamps onto each
/// report). It does NOT capture and does NOT implement the legacy `PointerDown`
/// / `PointerUp` send wire — a raw sink trades those for the raw stream.
#[derive(Debug, Default)]
struct RawPointerSink {
    /// Every raw button edge, in arrival order (append-only).
    reports: Vec<ButtonReport>,
    /// The live hover position fraction (from `pointer_move`), or `None` off the
    /// pane — stamped onto each report so a button edge carries WHERE it fired.
    x_frac: Option<f32>,
    y_frac: Option<f32>,
    /// R1423 — the live pointer PRESSURE (W3C `PointerEvent.pressure`), `0.0..=1.0`,
    /// from [`External::pointer_pressure`]. Drives the ink mark's diameter, so a
    /// pen / driven force paints a bigger dot — a pressure-aware surface.
    pressure: f32,
    /// R1429 — the live pointer TILT (W3C `PointerEvent.tiltX/tiltY`), in degrees,
    /// from [`External::pointer_tilt`]. Leans the pen-tip marker off the cursor,
    /// so a pen's angle reads on screen — a tilt-aware surface.
    tilt_x: f32,
    tilt_y: f32,
    /// R1430 — the remaining the toolkit tablet event scalar axes: twist
    /// (barrel rotation, orbits the orientation dot), tangential pressure
    /// (airbrush wheel, fills the bar), and hover height (the toolkit `z()`,
    /// shrinks the tip).
    twist: f32,
    tangential: f32,
    height: f32,
    /// R1431 — the producing device (W3C `pointerType`), colouring the pen-tip
    /// marker; a stylus flipping to its eraser end reads without a device query.
    kind: PointerKind,
}

impl RawPointerSink {
    fn new() -> Self {
        Self::default()
    }

    fn last(&self) -> Option<&ButtonReport> {
        self.reports.last()
    }
}

impl External for RawPointerSink {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Opt into the raw multi-button stream — the whole point of the demo.
    fn wants_raw_pointer_buttons(&self) -> bool {
        true
    }

    /// Opt into hover-move so the pointer POSITION is forwarded on a plain hover
    /// (no button held). The sink stamps this live fraction onto each button
    /// report — a raw sink correlates the button edge with the position it
    /// already tracks, exactly as a pane oracle maps a click to a terminal cell.
    fn wants_hover_move(&self) -> bool {
        true
    }

    fn pointer_move(&mut self, at: PointerReading) {
        self.x_frac = Some(at.u().clamp(0.0, 1.0));
        self.y_frac = Some(at.v().clamp(0.0, 1.0));
    }

    /// R1423 — record the live pointer pressure (W3C `PointerEvent.pressure`).
    /// The router forwards it alongside each move AND on a standalone
    /// `scene/pointer_pressure` change, so the ink mark reacts to a pen / driven
    /// force. A pressure-aware surface — the seam under test.
    fn pointer_pressure(&mut self, pressure: f32) {
        self.pressure = pressure.clamp(0.0, 1.0);
    }

    /// R1429 — record the live pointer tilt (W3C `PointerEvent.tiltX/tiltY`). The
    /// router forwards it alongside each move AND on a standalone
    /// `scene/pointer_tilt` change, so the pen-tip marker leans with the pen's
    /// angle. A tilt-aware surface — the seam under test.
    fn pointer_tilt(&mut self, tilt_x: f32, tilt_y: f32) {
        self.tilt_x = tilt_x.clamp(-90.0, 90.0);
        self.tilt_y = tilt_y.clamp(-90.0, 90.0);
    }

    /// R1430 — record the live pointer twist (W3C `PointerEvent.twist`), wrapped
    /// to `0.0..=360.0` (an angle). Orbits the orientation dot — a twist-aware
    /// surface.
    fn pointer_twist(&mut self, twist: f32) {
        self.twist = twist.rem_euclid(360.0);
    }

    /// R1430 — record the live tangential pressure (W3C
    /// `PointerEvent.tangentialPressure`), clamped to `-1.0..=1.0`. Fills the
    /// finger-wheel bar — an airbrush-aware surface.
    fn pointer_tangential_pressure(&mut self, tangential: f32) {
        self.tangential = tangential.clamp(-1.0, 1.0);
    }

    /// R1430 — record the live hover height (the toolkit `z()`), floored at
    /// `0.0`. Shrinks the pen-tip marker — a hover-height-aware surface.
    fn pointer_height(&mut self, height: f32) {
        self.height = height.max(0.0);
    }

    /// R1431 — record the producing device (W3C `pointerType`). Colours the
    /// pen-tip marker — a device-aware surface (an eraser flips the canvas).
    fn pointer_kind(&mut self, kind: PointerKind) {
        self.kind = kind;
    }

    /// Record one raw button edge with the modifiers held at that edge and the
    /// live position stamped in. This is the seam under test.
    fn raw_pointer_button(&mut self, event: RawPointerButton) {
        self.reports.push(ButtonReport {
            button: event.button,
            edge: event.edge,
            modifiers: event.modifiers,
            buttons: event.buttons,
            click_count: event.click_count,
            x_frac: self.x_frac,
            y_frac: self.y_frac,
        });
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for RawPointerSink {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // Total raw button edges received (monotone until `clear`).
                    SchemaField::new("report_count", "int"),
                    // The last report as "button:edge:mods", or Null when none.
                    SchemaField::new("last", "string"),
                    SchemaField::new("last_button", "string"),
                    SchemaField::new("last_edge", "string"),
                    SchemaField::new("last_mods", "string"),
                    // R1418 — the held-button set after the last edge, the
                    // toolkit `buttons()` peer, as an `lmr` wire token (e.g. "lr").
                    SchemaField::new("last_buttons", "string"),
                    // R1422 — the consecutive-click ordinal of the last edge,
                    // the toolkit `MouseButtonDblClick` peer (2 = a synthesised double).
                    SchemaField::new("last_clicks", "int"),
                    // The position fraction stamped at the last edge (Null off-pane).
                    SchemaField::new("last_x", "float"),
                    SchemaField::new("last_y", "float"),
                    // The live hover position fraction (Null off the pane).
                    SchemaField::new("x_frac", "float"),
                    SchemaField::new("y_frac", "float"),
                    // R1423 — the live pointer pressure (W3C `PointerEvent.pressure`),
                    // 0.0..=1.0; drives the ink mark's diameter.
                    SchemaField::new("pressure", "float"),
                    // R1429 — the live pointer tilt (W3C `PointerEvent.tiltX/tiltY`),
                    // in degrees -90..=90; leans the pen-tip marker off the cursor.
                    SchemaField::new("tilt_x", "float"),
                    SchemaField::new("tilt_y", "float"),
                    // R1430 — the remaining the toolkit tablet event scalar
                    // axes: twist (0..=360 deg), tangential (-1..=1), height
                    // (the toolkit z(), >= 0).
                    SchemaField::new("twist", "float"),
                    SchemaField::new("tangential", "float"),
                    SchemaField::new("height", "float"),
                    // R1431 — the producing device (W3C pointerType): one of
                    // mouse / pen / eraser / touch.
                    SchemaField::new("pointer_type", "string"),
                    // The full ";"-joined report sequence (empty string if none).
                    SchemaField::new("log", "string"),
                    // The router pointer boundary (Leave / Cancel clear the live
                    // position when the pointer leaves the pane).
                    SchemaField::action("send", "string"),
                    // Reset the log — the AI-first peer of clearing the pane.
                    SchemaField::action("clear", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let last = self.last();
        match path {
            "report_count" => Ok(IntrospectValue::Int(
                i64::try_from(self.reports.len()).unwrap_or(i64::MAX),
            )),
            "last" => Ok(last.map_or(IntrospectValue::Null, |r| IntrospectValue::Text(r.label()))),
            "last_button" => Ok(last.map_or(IntrospectValue::Null, |r| {
                IntrospectValue::Text(r.button.as_wire_name().to_owned())
            })),
            "last_edge" => Ok(last.map_or(IntrospectValue::Null, |r| {
                IntrospectValue::Text(r.edge.as_wire_name().to_owned())
            })),
            "last_mods" => Ok(last.map_or(IntrospectValue::Null, |r| {
                IntrospectValue::Text(r.modifiers.as_wire_token())
            })),
            "last_buttons" => Ok(last.map_or(IntrospectValue::Null, |r| {
                IntrospectValue::Text(r.buttons.as_wire_token())
            })),
            "last_clicks" => Ok(last.map_or(IntrospectValue::Null, |r| {
                IntrospectValue::Int(i64::from(r.click_count))
            })),
            "last_x" => Ok(last
                .and_then(|r| r.x_frac)
                .map_or(IntrospectValue::Null, |f| IntrospectValue::Float(f.into()))),
            "last_y" => Ok(last
                .and_then(|r| r.y_frac)
                .map_or(IntrospectValue::Null, |f| IntrospectValue::Float(f.into()))),
            "x_frac" => Ok(self
                .x_frac
                .map_or(IntrospectValue::Null, |f| IntrospectValue::Float(f.into()))),
            "y_frac" => Ok(self
                .y_frac
                .map_or(IntrospectValue::Null, |f| IntrospectValue::Float(f.into()))),
            "pressure" => Ok(IntrospectValue::Float(self.pressure.into())),
            "tilt_x" => Ok(IntrospectValue::Float(self.tilt_x.into())),
            "tilt_y" => Ok(IntrospectValue::Float(self.tilt_y.into())),
            "twist" => Ok(IntrospectValue::Float(self.twist.into())),
            "tangential" => Ok(IntrospectValue::Float(self.tangential.into())),
            "height" => Ok(IntrospectValue::Float(self.height.into())),
            "pointer_type" => Ok(IntrospectValue::Text(self.kind.as_wire_name().to_owned())),
            "log" => Ok(IntrospectValue::Text(
                self.reports
                    .iter()
                    .map(ButtonReport::label)
                    .collect::<Vec<_>>()
                    .join(";"),
            )),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Every field is a read-only projection of the input log.
            "report_count" | "last" | "last_button" | "last_edge" | "last_mods"
            | "last_buttons" | "last_clicks" | "last_x" | "last_y" | "x_frac" | "y_frac"
            | "pressure" | "tilt_x" | "tilt_y" | "twist" | "tangential" | "height"
            | "pointer_type" | "log" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // The router pointer boundary. Leaving the pane (or a cancel) clears
            // the LIVE position — button reports are the log and stay recorded.
            "send" => {
                if let IntrospectValue::Text(ref name) = args {
                    if matches!(name.as_str(), "PointerLeave" | "PointerCancel") {
                        self.x_frac = None;
                        self.y_frac = None;
                    }
                }
                Ok(IntrospectValue::Null)
            }
            // Reset the report log (the AI-first peer of clearing the pane).
            "clear" => {
                self.reports.clear();
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The binding -----------------------------------------------------------

struct RawPointerView;

impl WidgetCore for RawPointerView {
    type State = SinkState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(RawPointerSink::new())
    }

    fn tag() -> &'static str {
        PANE_TAG
    }

    fn read_state(scene: &Scene) -> SinkState {
        read_sink(scene)
    }

    fn view(state: SinkState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-raw-pointer (R1416 §5.35 raw multi-button pointer sink)"
    }

    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &SinkState) -> String {
        readout_text(state)
    }
}

impl WidgetA11y for RawPointerView {
    fn access_node(state: &SinkState, _focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(PANE_TAG, AriaRole::Group)
                .with_name("Raw pointer sink")
                .with_value(AccessValue::Text(readout_text(state))),
        ]
    }
}

impl WidgetView for RawPointerView {
    type Renderer = HelloRawPointerRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<RawPointerView>();
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

    fn rendered(state: SinkState) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, &Frame::new()))
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::empty()
        }
    }

    fn edge(
        sink: &mut RawPointerSink,
        button: PointerButton,
        edge: PointerEdge,
        modifiers: Modifiers,
    ) {
        // Single-edge helper (no chord): a press holds its button, a release
        // holds nothing — the toolkit `buttons()` state the router would compute.
        let buttons = match edge {
            PointerEdge::Down => PointerButtons::empty().with(button),
            PointerEdge::Up => PointerButtons::empty(),
        };
        sink.raw_pointer_button(RawPointerButton {
            button,
            edge,
            modifiers,
            buttons,
            // Single edges in this helper — the router would synthesise the count;
            // the manual helper reports a plain first click.
            click_count: 1,
        });
    }

    #[test]
    fn boot_shows_the_prompt_and_no_report() {
        let scene = rendered(SinkState::default());
        let Some(Scene::Text(t)) = find(&scene, READOUT_TAG) else {
            panic!("readout line")
        };
        assert!(
            t.content.contains("press any mouse button"),
            "idle prompt shown, got {:?}",
            t.content
        );
        assert!(
            find(&scene, PANE_TAG).is_some(),
            "the pane surface is present"
        );
    }

    #[test]
    fn the_sink_opts_into_raw_buttons_and_hover_not_capture() {
        let sink = RawPointerSink::new();
        assert!(
            sink.wants_raw_pointer_buttons(),
            "opts into the raw multi-button stream (the R1416 seam)"
        );
        assert!(
            sink.wants_hover_move(),
            "opts into hover-move for the position it stamps onto reports"
        );
        assert!(
            !sink.wants_pointer_capture(),
            "does NOT capture — the pane resolves each edge via hover"
        );
    }

    #[test]
    fn the_held_button_set_is_recorded_and_exposed() {
        // R1418 — the sink stores and exposes the toolkit `buttons()` held set the
        // router hands it (here a {left, right} chord), so an AI client reads
        // WHICH buttons are down, not only the one that changed.
        let mut sink = RawPointerSink::new();
        sink.raw_pointer_button(RawPointerButton {
            button: PointerButton::Right,
            edge: PointerEdge::Down,
            modifiers: Modifiers::empty(),
            buttons: PointerButtons::empty()
                .with(PointerButton::Left)
                .with(PointerButton::Right),
            click_count: 1,
        });
        assert_eq!(
            sink.query("last_buttons"),
            Ok(IntrospectValue::Text("lr".to_owned())),
            "the held set is exposed as an lmr wire token"
        );
    }

    #[test]
    fn the_double_click_count_is_exposed_and_badged() {
        // R1422 — the router synthesises click_count = 2 on a double-click; the
        // sink exposes it as `last_clicks` and the readout badges it "×2" so the
        // double is visible as data AND on screen.
        let mut sink = RawPointerSink::new();
        sink.pointer_move(PointerReading::over_unit((0.5, 0.5)));
        sink.raw_pointer_button(RawPointerButton {
            button: PointerButton::Left,
            edge: PointerEdge::Down,
            modifiers: Modifiers::empty(),
            buttons: PointerButtons::empty().with(PointerButton::Left),
            click_count: 2,
        });
        assert_eq!(
            sink.query("last_clicks"),
            Ok(IntrospectValue::Int(2)),
            "the double-click count is exposed as `last_clicks`"
        );
        let state = SinkState {
            count: 2,
            last: sink.last().copied(),
            x_frac: Some(0.5),
            y_frac: Some(0.5),
            pressure: 0.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            twist: 0.0,
            tangential: 0.0,
            height: 0.0,
            kind: PointerKind::Mouse,
        };
        assert!(
            readout_text(&state).contains("×2"),
            "the readout badges the double-click, got {:?}",
            readout_text(&state)
        );
    }

    #[test]
    fn a_single_click_is_not_badged() {
        // A plain first click (click_count = 1) carries no "×N" badge.
        let mut sink = RawPointerSink::new();
        sink.pointer_move(PointerReading::over_unit((0.5, 0.5)));
        edge(
            &mut sink,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        assert_eq!(sink.query("last_clicks"), Ok(IntrospectValue::Int(1)));
        let state = SinkState {
            count: 1,
            last: sink.last().copied(),
            x_frac: Some(0.5),
            y_frac: Some(0.5),
            pressure: 0.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            twist: 0.0,
            tangential: 0.0,
            height: 0.0,
            kind: PointerKind::Mouse,
        };
        assert!(
            !readout_text(&state).contains('×'),
            "a single click is not badged, got {:?}",
            readout_text(&state)
        );
    }

    #[test]
    fn pressure_is_stored_and_exposed() {
        // R1423 — the router forwards the W3C pressure via `pointer_pressure`;
        // the sink stores it, clamps to 0..1, and exposes `pressure`.
        let mut sink = RawPointerSink::new();
        sink.pointer_pressure(0.6);
        assert_eq!(
            sink.query("pressure"),
            Ok(IntrospectValue::Float(0.6_f32.into())),
            "the live pressure is exposed"
        );
        sink.pointer_pressure(5.0); // out of range → clamped
        assert_eq!(
            sink.query("pressure"),
            Ok(IntrospectValue::Float(1.0_f32.into())),
            "pressure clamps to 1.0"
        );
        assert_eq!(
            sink.intervene("pressure", IntrospectValue::Null),
            Err(InterveneError::ReadOnly),
            "pressure is a read-only projection of the input stream"
        );
    }

    /// The ink dot's pixel width from the rendered view, or `None` when the
    /// pressure mark is absent.
    fn ink_dot_width(state: SinkState) -> Option<u32> {
        let scene = rendered(state);
        match find(&scene, INK_TAG) {
            Some(Scene::Box(b)) => match b.layout.size.width {
                pinion_core::style::SizeValue::Px(w) => Some(w),
                _ => None,
            },
            _ => None,
        }
    }

    #[test]
    fn the_ink_dot_appears_only_under_pressure_and_grows_with_it() {
        // R1423 — a pressure-reactive ink mark: absent at zero force, present and
        // wider as pressure rises (the DCC ink-brush vignette).
        let base = SinkState {
            x_frac: Some(0.5),
            y_frac: Some(0.5),
            ..SinkState::default()
        };
        assert_eq!(
            ink_dot_width(SinkState {
                pressure: 0.0,
                ..base
            }),
            None,
            "no ink mark at zero pressure — a plain hover leaves no mark"
        );
        let light = ink_dot_width(SinkState {
            pressure: 0.2,
            ..base
        })
        .expect("a mark under light pressure");
        let heavy = ink_dot_width(SinkState {
            pressure: 0.9,
            ..base
        })
        .expect("a mark under heavy pressure");
        assert!(
            heavy > light,
            "the mark grows with pressure: heavy {heavy} must exceed light {light}"
        );
    }

    #[test]
    fn the_readout_surfaces_the_live_pressure() {
        // R1423 — the readout names the live pressure so a pen force reads as data.
        let mut sink = RawPointerSink::new();
        sink.pointer_move(PointerReading::over_unit((0.4, 0.4)));
        edge(
            &mut sink,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        let state = SinkState {
            count: 1,
            last: sink.last().copied(),
            x_frac: Some(0.4),
            y_frac: Some(0.4),
            pressure: 0.75,
            tilt_x: 0.0,
            tilt_y: 0.0,
            twist: 0.0,
            tangential: 0.0,
            height: 0.0,
            kind: PointerKind::Mouse,
        };
        assert!(
            readout_text(&state).contains("pressure 0.75"),
            "the readout names the pressure, got {:?}",
            readout_text(&state)
        );
    }

    #[test]
    fn tilt_is_stored_clamped_and_exposed() {
        // R1429 — the router forwards the W3C tilt via `pointer_tilt`; the sink
        // stores both axes, clamps each to -90..=90 degrees, and exposes them.
        let mut sink = RawPointerSink::new();
        sink.pointer_tilt(30.0, -45.0);
        assert_eq!(
            sink.query("tilt_x"),
            Ok(IntrospectValue::Float(30.0_f32.into())),
            "the live tilt_x is exposed"
        );
        assert_eq!(
            sink.query("tilt_y"),
            Ok(IntrospectValue::Float((-45.0_f32).into())),
            "the live tilt_y is exposed"
        );
        sink.pointer_tilt(120.0, -120.0); // out of range → clamped to the axis limits
        assert_eq!(
            sink.query("tilt_x"),
            Ok(IntrospectValue::Float(90.0_f32.into())),
            "tilt_x clamps to +90"
        );
        assert_eq!(
            sink.query("tilt_y"),
            Ok(IntrospectValue::Float((-90.0_f32).into())),
            "tilt_y clamps to -90"
        );
        for path in ["tilt_x", "tilt_y"] {
            assert_eq!(
                sink.intervene(path, IntrospectValue::Null),
                Err(InterveneError::ReadOnly),
                "{path} is a read-only projection of the input stream"
            );
        }
    }

    /// The tilt tip marker's absolute `(x, y)` position from the rendered view,
    /// or `None` when the marker is absent (no known cursor position).
    fn tip_pos(state: SinkState) -> Option<(u32, u32)> {
        let scene = rendered(state);
        match find(&scene, TIP_TAG) {
            Some(Scene::Box(b)) => b.layout.absolute_position,
            _ => None,
        }
    }

    #[test]
    fn the_tilt_tip_leans_off_the_cursor_and_needs_a_position() {
        // R1429 — the pen-tip marker sits ON the cursor at zero tilt, leans right
        // as tilt_x rises and down as tilt_y rises (the W3C tiltX/tiltY sign), and
        // is absent with no known position (the pen off the pane).
        let base = SinkState {
            x_frac: Some(0.5),
            y_frac: Some(0.5),
            ..SinkState::default()
        };
        let (x0, y0) = tip_pos(base).expect("a tip on hover, even at zero tilt");
        let (xr, yr) = tip_pos(SinkState {
            tilt_x: 60.0,
            ..base
        })
        .expect("a tip under rightward tilt");
        assert!(xr > x0, "tilt_x>0 leans the tip right: {xr} > {x0}");
        assert_eq!(yr, y0, "a pure tilt_x does not move the tip vertically");
        let (xd, yd) = tip_pos(SinkState {
            tilt_y: 60.0,
            ..base
        })
        .expect("a tip under downward tilt");
        assert!(yd > y0, "tilt_y>0 leans the tip down: {yd} > {y0}");
        assert_eq!(xd, x0, "a pure tilt_y does not move the tip horizontally");
        // A negative tilt_x leans the tip the other way (left of centre).
        let (xl, _) = tip_pos(SinkState {
            tilt_x: -60.0,
            ..base
        })
        .expect("a tip under leftward tilt");
        assert!(xl < x0, "tilt_x<0 leans the tip left: {xl} < {x0}");
        // No known position → no marker (a pen lifted off the pane).
        assert_eq!(
            tip_pos(SinkState {
                tilt_x: 60.0,
                ..SinkState::default()
            }),
            None,
            "no tip without a cursor position"
        );
    }

    #[test]
    fn the_readout_surfaces_the_live_tilt() {
        // R1429 — the readout names the live tilt so a pen's lean reads as data,
        // and only when the pen is off the perpendicular.
        let mut sink = RawPointerSink::new();
        sink.pointer_move(PointerReading::over_unit((0.4, 0.4)));
        edge(
            &mut sink,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        let leaning = SinkState {
            count: 1,
            last: sink.last().copied(),
            x_frac: Some(0.4),
            y_frac: Some(0.4),
            pressure: 0.0,
            tilt_x: 30.0,
            tilt_y: -20.0,
            twist: 0.0,
            tangential: 0.0,
            height: 0.0,
            kind: PointerKind::Mouse,
        };
        assert!(
            readout_text(&leaning).contains("tilt (30\u{b0}, -20\u{b0})"),
            "the readout names the tilt, got {:?}",
            readout_text(&leaning)
        );
        let upright = SinkState {
            tilt_x: 0.0,
            tilt_y: 0.0,
            ..leaning
        };
        assert!(
            !readout_text(&upright).contains("tilt"),
            "an upright pen shows no tilt badge, got {:?}",
            readout_text(&upright)
        );
    }

    #[test]
    fn r1430_scalar_axes_store_clamped_and_exposed() {
        // R1430 — twist WRAPS to 0..360 (an angle), tangential CLAMPS to -1..1,
        // height FLOORS at 0, and each is a read-only projection of the stream.
        let mut sink = RawPointerSink::new();
        sink.pointer_twist(400.0); // 400 -> 40 (wrapped)
        sink.pointer_tangential_pressure(2.0); // 2 -> 1 (clamped)
        sink.pointer_height(-5.0); // -5 -> 0 (floored)
        assert_eq!(
            sink.query("twist"),
            Ok(IntrospectValue::Float(40.0_f32.into())),
            "twist wraps into 0..360"
        );
        assert_eq!(
            sink.query("tangential"),
            Ok(IntrospectValue::Float(1.0_f32.into())),
            "tangential clamps to 1.0"
        );
        assert_eq!(
            sink.query("height"),
            Ok(IntrospectValue::Float(0.0_f32.into())),
            "height floors at 0.0"
        );
        for path in ["twist", "tangential", "height"] {
            assert_eq!(
                sink.intervene(path, IntrospectValue::Null),
                Err(InterveneError::ReadOnly),
                "{path} is a read-only projection of the input stream"
            );
        }
    }

    /// The `w` of the tangential fill bar from the rendered view.
    fn tang_bar_width(state: SinkState) -> u32 {
        let scene = rendered(state);
        match find(&scene, TANG_TAG) {
            Some(Scene::Box(b)) => match b.layout.size.width {
                pinion_core::style::SizeValue::Px(w) => w,
                _ => 0,
            },
            _ => 0,
        }
    }

    #[test]
    fn r1430_the_tang_bar_fills_with_tangential_pressure() {
        // R1430 — the finger-wheel bar is half-full at rest (tangential 0), empty
        // toward -1, and full toward +1 (the airbrush wheel gauge).
        let base = SinkState {
            x_frac: Some(0.5),
            y_frac: Some(0.5),
            ..SinkState::default()
        };
        let rest = tang_bar_width(base);
        let low = tang_bar_width(SinkState {
            tangential: -1.0,
            ..base
        });
        let high = tang_bar_width(SinkState {
            tangential: 1.0,
            ..base
        });
        assert!(
            low < rest,
            "negative tangential empties the bar: {low} < {rest}"
        );
        assert!(
            high > rest,
            "positive tangential fills the bar: {high} > {rest}"
        );
    }

    #[test]
    fn r1430_the_twist_orbit_circles_the_pen_tip() {
        // R1430 — the orientation dot orbits the tip: twist 0 sits ABOVE the tip,
        // twist 90 to its RIGHT (clockwise), so barrel rotation reads on screen.
        let base = SinkState {
            x_frac: Some(0.5),
            y_frac: Some(0.5),
            ..SinkState::default()
        };
        let (tx, ty) = tip_pos(base).expect("a tip on hover");
        let orbit_pos = |twist: f32| -> (u32, u32) {
            let scene = rendered(SinkState { twist, ..base });
            match find(&scene, ORBIT_TAG) {
                Some(Scene::Box(b)) => b.layout.absolute_position.expect("orbit position"),
                _ => panic!("no orbit dot"),
            }
        };
        let (_ux, uy) = orbit_pos(0.0);
        assert!(uy < ty, "twist 0 orbits above the tip: {uy} < {ty}");
        let (rx, ry) = orbit_pos(90.0);
        assert!(rx > tx, "twist 90 orbits right of the tip: {rx} > {tx}");
        assert!(ry > uy, "twist 90 sits lower than twist 0 (clockwise)");
    }

    #[test]
    fn r1430_the_pen_tip_shrinks_as_the_pen_lifts() {
        // R1430 — the tip marker shrinks with hover height (the toolkit z()):
        // largest in contact, smaller as the pen lifts off the surface.
        let base = SinkState {
            x_frac: Some(0.5),
            y_frac: Some(0.5),
            ..SinkState::default()
        };
        let tip_size = |height: f32| -> u32 {
            let scene = rendered(SinkState { height, ..base });
            match find(&scene, TIP_TAG) {
                Some(Scene::Box(b)) => match b.layout.size.width {
                    pinion_core::style::SizeValue::Px(w) => w,
                    _ => 0,
                },
                _ => 0,
            }
        };
        let contact = tip_size(0.0);
        let lifted = tip_size(HEIGHT_FULL_PX);
        assert!(
            lifted < contact,
            "a lifted pen paints a smaller tip: {lifted} < {contact}"
        );
    }

    #[test]
    fn r1430_the_readout_surfaces_the_scalar_axes() {
        // R1430 — the readout names twist / tangential / height when off rest.
        let mut sink = RawPointerSink::new();
        sink.pointer_move(PointerReading::over_unit((0.4, 0.4)));
        edge(
            &mut sink,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        let state = SinkState {
            count: 1,
            last: sink.last().copied(),
            x_frac: Some(0.4),
            y_frac: Some(0.4),
            pressure: 0.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            twist: 45.0,
            tangential: -0.5,
            height: 2.0,
            kind: PointerKind::Mouse,
        };
        let text = readout_text(&state);
        assert!(text.contains("twist 45\u{b0}"), "names twist, got {text:?}");
        assert!(
            text.contains("tang -0.50"),
            "names tangential, got {text:?}"
        );
        assert!(text.contains("z 2.0"), "names height, got {text:?}");
    }

    #[test]
    fn r1431_pointer_kind_stores_reads_colours_and_badges() {
        // R1431 — the device kind stores, exposes as `pointer_type`, is read-only,
        // colours the tip distinctly per device, and badges the readout.
        let mut sink = RawPointerSink::new();
        assert_eq!(
            sink.query("pointer_type"),
            Ok(IntrospectValue::Text("mouse".to_owned())),
            "the default device is a mouse"
        );
        sink.pointer_kind(PointerKind::Eraser);
        assert_eq!(
            sink.query("pointer_type"),
            Ok(IntrospectValue::Text("eraser".to_owned())),
            "the eraser device is exposed"
        );
        assert_eq!(
            sink.intervene("pointer_type", IntrospectValue::Null),
            Err(InterveneError::ReadOnly),
            "pointer_type is a read-only projection of the input stream"
        );

        // The tip colour distinguishes the devices (pen != eraser != mouse).
        let owner = Owner::new();
        let (pen, eraser, mouse) = owner.run(|| {
            let t = use_theme(THEME_TAG).theme_animated();
            (
                tip_color(PointerKind::Pen, &t),
                tip_color(PointerKind::Eraser, &t),
                tip_color(PointerKind::Mouse, &t),
            )
        });
        assert_ne!(pen, eraser, "a pen and an eraser paint different tips");
        assert_ne!(pen, mouse, "a pen and a mouse paint different tips");

        // The readout badges a non-mouse device, but not a plain mouse.
        sink.pointer_move(PointerReading::over_unit((0.4, 0.4)));
        edge(
            &mut sink,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        let pen_state = SinkState {
            count: 1,
            last: sink.last().copied(),
            x_frac: Some(0.4),
            y_frac: Some(0.4),
            kind: PointerKind::Pen,
            ..SinkState::default()
        };
        assert!(
            readout_text(&pen_state).contains("\u{b7} pen"),
            "the readout badges the pen, got {:?}",
            readout_text(&pen_state)
        );
        let mouse_state = SinkState {
            kind: PointerKind::Mouse,
            ..pen_state
        };
        assert!(
            !readout_text(&mouse_state).contains("mouse"),
            "a plain mouse is not badged, got {:?}",
            readout_text(&mouse_state)
        );
    }

    #[test]
    fn every_button_and_edge_is_recorded_with_button_identity() {
        let mut sink = RawPointerSink::new();
        for (b, e) in [
            (PointerButton::Left, PointerEdge::Down),
            (PointerButton::Left, PointerEdge::Up),
            (PointerButton::Middle, PointerEdge::Down),
            (PointerButton::Middle, PointerEdge::Up),
            (PointerButton::Right, PointerEdge::Down),
            (PointerButton::Right, PointerEdge::Up),
        ] {
            edge(&mut sink, b, e, Modifiers::empty());
        }
        assert_eq!(
            sink.query("log"),
            Ok(IntrospectValue::Text(
                "left:down:;left:up:;middle:down:;middle:up:;right:down:;right:up:".to_owned()
            )),
            "all three buttons on both edges, each identified"
        );
        assert_eq!(sink.query("report_count"), Ok(IntrospectValue::Int(6)));
    }

    #[test]
    fn the_press_edge_carries_modifiers() {
        // Gap B: the legacy PointerDown send wire dropped press modifiers; the
        // raw stream carries them on the DOWN edge.
        let mut sink = RawPointerSink::new();
        edge(&mut sink, PointerButton::Right, PointerEdge::Down, shift());
        assert_eq!(
            sink.query("last"),
            Ok(IntrospectValue::Text("right:down:s".to_owned())),
            "the right PRESS carries the Shift modifier"
        );
        assert_eq!(
            sink.query("last_mods"),
            Ok(IntrospectValue::Text("s".to_owned()))
        );
    }

    #[test]
    fn the_position_is_stamped_onto_each_report() {
        let mut sink = RawPointerSink::new();
        sink.pointer_move(PointerReading::over_unit((0.25, 0.75)));
        edge(
            &mut sink,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        assert_eq!(
            sink.query("last_x"),
            Ok(IntrospectValue::Float(0.25_f32.into())),
            "the report stamps the live hover x"
        );
        assert_eq!(
            sink.query("last_y"),
            Ok(IntrospectValue::Float(0.75_f32.into()))
        );
    }

    #[test]
    fn leaving_the_pane_clears_the_live_position_but_not_the_log() {
        let mut sink = RawPointerSink::new();
        sink.pointer_move(PointerReading::over_unit((0.5, 0.5)));
        edge(
            &mut sink,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        sink.invoke("send", IntrospectValue::Text("PointerLeave".to_owned()))
            .expect("send is infallible");
        assert_eq!(
            sink.query("x_frac"),
            Ok(IntrospectValue::Null),
            "the live position clears on leave"
        );
        assert_eq!(
            sink.query("report_count"),
            Ok(IntrospectValue::Int(1)),
            "the recorded reports survive a leave"
        );
    }

    #[test]
    fn clear_resets_the_log() {
        let mut sink = RawPointerSink::new();
        edge(
            &mut sink,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        assert_eq!(sink.query("report_count"), Ok(IntrospectValue::Int(1)));
        sink.invoke("clear", IntrospectValue::Null)
            .expect("clear is infallible");
        assert_eq!(sink.query("report_count"), Ok(IntrospectValue::Int(0)));
        assert_eq!(sink.query("last"), Ok(IntrospectValue::Null));
    }

    #[test]
    fn every_field_is_read_only() {
        let mut sink = RawPointerSink::new();
        for path in [
            "report_count",
            "last",
            "last_button",
            "last_mods",
            "last_buttons",
            "last_clicks",
            "x_frac",
            "log",
        ] {
            assert_eq!(
                sink.intervene(path, IntrospectValue::Null),
                Err(InterveneError::ReadOnly),
                "{path} is a read-only projection of the input log"
            );
        }
        assert_eq!(
            sink.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath)
        );
    }

    #[test]
    fn the_readout_names_the_last_report() {
        let mut sink = RawPointerSink::new();
        sink.pointer_move(PointerReading::over_unit((0.3, 0.4)));
        edge(
            &mut sink,
            PointerButton::Middle,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        let state = SinkState {
            count: 1,
            last: sink.last().copied(),
            x_frac: Some(0.3),
            y_frac: Some(0.4),
            pressure: 0.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            twist: 0.0,
            tangential: 0.0,
            height: 0.0,
            kind: PointerKind::Mouse,
        };
        let scene = rendered(state);
        let Some(Scene::Text(t)) = find(&scene, READOUT_TAG) else {
            panic!("readout line")
        };
        assert!(
            t.content.contains("middle:down:"),
            "readout names the last report, got {:?}",
            t.content
        );
    }
}
