//! R1433 §5.35 §5.15 — a native ROTATION gesture spins a gizmo.
//!
//! A `RotationGizmo` [`External`] overrides [`External::rotation_gesture`] — the toolkit native gesture event `RotateNativeGesture` / macOS
//! `rotateWithEvent:` peer — and accumulates the INCREMENTAL degrees of a two-finger trackpad
//! twist across the [`GesturePhase::Begin`]`..`[`End`](GesturePhase::End) arc: `angle += rotation` per event
//! (winit's sign convention, positive = counter-clockwise). The whole point of
//! the phase is the arc bracket — on [`GesturePhase::Cancel`] the gizmo DISCARDS the in-flight
//! rotation and reverts to the angle it held when the gesture began, exactly
//! as a preview drops when the fingers lift without committing.
//!
//! winit surfaces `WindowEvent::RotationGesture` only on macOS / iOS, so the
//! `scene/rotation_gesture` RPC is the AI-first driver (§2 #2): a headless client
//! rotates the gizmo with no trackpad, and reads the live angle back through the
//! introspect surface (§2 #7) — the needle's paint rect and the `angle_deg`
//! field move together.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{Border, BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, GesturePhase, Modifiers, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRotateGestureRenderer, HelloRotateGestureRendererError);

const WIN_W: u32 = 640;
const WIN_H: u32 = 440;
const THEME_TAG: &str = "app";

/// The gizmo's paint tag **and** the [`RotationGizmo`]'s registration tag —
/// addressed over RPC as `/external/<field>`. A transparent, pointer-opaque
/// capture surface over the pane carries it, so a rotation anywhere over the
/// pane resolves to the gizmo (the router offers a native gesture to the widget
/// under the cursor).
const PANE_TAG: &str = "gizmo";

/// The rotating needle-tip's paint tag. A snapshot reads its rect to confirm the
/// tip orbits the hub as the accumulated angle changes.
const NEEDLE_TAG: &str = "rotate.needle";

/// The human-readable report line at the window's foot.
const READOUT_TAG: &str = "rotate.readout";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 13;

/// Window-absolute pane region. The transparent capture surface covers exactly
/// this rect, so a rotation's `x_rel` / `y_rel` fraction `0.0..=1.0` is the
/// anchor position on the gizmo.
const PANE_RECT: Rect = Rect::new(20, 56, WIN_W - 40, WIN_H - 120);

/// The dial hub — the fixed centre the needle orbits (window-absolute).
const HUB_X: f64 = PANE_RECT.x as f64 + PANE_RECT.w as f64 / 2.0;
const HUB_Y: f64 = PANE_RECT.y as f64 + PANE_RECT.h as f64 / 2.0;

/// The needle-tip orbit radius (px) and the tip marker's side (px).
const NEEDLE_R: f64 = 100.0;
const NEEDLE_SIDE: u32 = 16;

// --- The paint-facing state snapshot ---------------------------------------

/// The read-only projection the binding paints, read back off the
/// [`RotationGizmo`]'s introspect surface — the SAME fields the AI-first
/// `scene/query` client sees, so the picture and the data can never disagree.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ViewState {
    /// The accumulated rotation in degrees (`0.0` = the base orientation).
    angle_deg: f64,
    /// The last gesture phase, or `None` before the first rotation.
    phase: Option<GesturePhase>,
    /// The number of rotation events received (monotone until `reset`).
    events: u64,
    /// The anchor fraction of the last rotation (`None` before the first).
    anchor: Option<(f32, f32)>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            angle_deg: 0.0,
            phase: None,
            events: 0,
            anchor: None,
        }
    }
}

/// The idle prompt / live report line — the SSOT both the status text and the
/// a11y value read.
fn readout_text(state: &ViewState) -> String {
    match state.phase {
        None => "twist to rotate the needle (scene/rotation_gesture drives it headless)".to_owned(),
        Some(phase) => {
            let anchor = match state.anchor {
                Some((x, y)) => format!(" @ ({x:.2}, {y:.2})"),
                None => String::new(),
            };
            format!(
                "angle {:.1}° — {} — {} event(s){anchor}",
                state.angle_deg,
                phase.as_wire_name(),
                state.events,
            )
        }
    }
}

/// The needle-tip's window-absolute rect for an accumulated `angle_deg`. The tip
/// orbits the hub at [`NEEDLE_R`]: `x = hub_x + R·cos θ`, `y = hub_y − R·sin θ`
/// (screen y grows downward, so the minus makes a positive — counter-clockwise —
/// angle lift the tip). This paint rect is what a `scene/snapshot` reads to
/// confirm the twist rotated the gizmo, not only the introspect field.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the tip stays inside the pane (hub ± R), a small non-negative pixel count; the fractional part is not meaningful for a paint rect"
)]
fn needle_rect(angle_deg: f64) -> Rect {
    let rad = angle_deg.to_radians();
    let half = f64::from(NEEDLE_SIDE) / 2.0;
    let cx = HUB_X + NEEDLE_R * rad.cos();
    let cy = HUB_Y - NEEDLE_R * rad.sin();
    let x = (cx - half).round().clamp(0.0, f64::from(u32::MAX)) as u32;
    let y = (cy - half).round().clamp(0.0, f64::from(u32::MAX)) as u32;
    Rect::new(x, y, NEEDLE_SIDE, NEEDLE_SIDE)
}

// --- The rotation gizmo (primary External) ---------------------------------

/// The rotation authority. Overrides [`External::rotation_gesture`] and
/// accumulates the incremental degrees across the gesture arc, reverting on
/// cancel.
#[derive(Debug)]
struct RotationGizmo {
    /// The accumulated rotation in degrees (unbounded — rotation is periodic).
    angle_deg: f64,
    /// The angle captured at the last [`GesturePhase::Begin`], restored on a
    /// [`GesturePhase::Cancel`] so an aborted twist discards its whole arc.
    angle_at_begin: f64,
    /// The last incremental rotation delta (the raw `rotation_gesture` value).
    rotation: f64,
    /// The last gesture phase, or `None` before the first rotation.
    phase: Option<GesturePhase>,
    /// The number of rotation events received (monotone until `reset`).
    events: u64,
    /// The anchor fraction of the last rotation (`None` before the first).
    anchor: Option<(f32, f32)>,
    /// The modifiers held on the last rotation (the toolkit-parity `Shift`-snap
    /// bit).
    modifiers: Modifiers,
}

impl Default for RotationGizmo {
    fn default() -> Self {
        Self {
            angle_deg: 0.0,
            angle_at_begin: 0.0,
            rotation: 0.0,
            phase: None,
            events: 0,
            anchor: None,
            modifiers: Modifiers::empty(),
        }
    }
}

impl RotationGizmo {
    fn new() -> Self {
        Self::default()
    }

    /// Fold one incremental rotation delta into the accumulated angle:
    /// `angle += rotation` (no clamp — a full turn wraps naturally in the paint).
    fn apply_rotation(&mut self, rotation: f64) {
        self.angle_deg += rotation;
    }
}

impl External for RotationGizmo {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// R1433 — the seam under test. Accumulate the incremental degrees across the
    /// `Begin..End` arc; snapshot the angle on `Begin` so `Cancel` can revert the
    /// whole arc; commit (keep the angle) on `End`. Always consumes — the gizmo
    /// owns the rotation (a native gesture has no fallback to decline to).
    fn rotation_gesture(
        &mut self,
        x_rel: f32,
        y_rel: f32,
        rotation: f64,
        phase: GesturePhase,
        modifiers: Modifiers,
    ) -> bool {
        self.events += 1;
        self.anchor = Some((x_rel.clamp(0.0, 1.0), y_rel.clamp(0.0, 1.0)));
        self.phase = Some(phase);
        self.rotation = rotation;
        self.modifiers = modifiers;
        match phase {
            GesturePhase::Begin => {
                self.angle_at_begin = self.angle_deg;
                self.apply_rotation(rotation);
            }
            GesturePhase::Update => self.apply_rotation(rotation),
            GesturePhase::End => {}
            GesturePhase::Cancel => self.angle_deg = self.angle_at_begin,
        }
        true
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for RotationGizmo {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The accumulated rotation in degrees (0.0 = base); drives the needle.
                    SchemaField::new("angle_deg", "float"),
                    // The angle captured at the last Begin (Cancel reverts to it).
                    SchemaField::new("angle_at_begin", "float"),
                    // The last incremental rotation delta (degrees).
                    SchemaField::new("rotation", "float"),
                    // The last GesturePhase: begin / update / end / cancel (Null yet).
                    SchemaField::new("phase", "string"),
                    // The rotation event count (monotone until `reset`).
                    SchemaField::new("events", "int"),
                    // The anchor fraction of the last rotation (Null before the first).
                    SchemaField::new("anchor_x", "float"),
                    SchemaField::new("anchor_y", "float"),
                    // The modifiers held at the last rotation, as a wire token (e.g. "s").
                    SchemaField::new("last_mods", "string"),
                    // Reset the rotation — the AI-first peer of a snap-to-zero.
                    SchemaField::new("reset", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "angle_deg" => Some(IntrospectValue::Float(self.angle_deg)),
            "angle_at_begin" => Some(IntrospectValue::Float(self.angle_at_begin)),
            "rotation" => Some(IntrospectValue::Float(self.rotation)),
            "phase" => Some(self.phase.map_or(IntrospectValue::Null, |p| {
                IntrospectValue::Text(p.as_wire_name().to_owned())
            })),
            "events" => Some(IntrospectValue::Int(
                i64::try_from(self.events).unwrap_or(i64::MAX),
            )),
            "anchor_x" => Some(self.anchor.map_or(IntrospectValue::Null, |(x, _)| {
                IntrospectValue::Float(x.into())
            })),
            "anchor_y" => Some(self.anchor.map_or(IntrospectValue::Null, |(_, y)| {
                IntrospectValue::Float(y.into())
            })),
            "last_mods" => Some(IntrospectValue::Text(self.modifiers.as_wire_token())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Every field is a read-only projection of the gesture stream.
            "angle_deg" | "angle_at_begin" | "rotation" | "phase" | "events" | "anchor_x"
            | "anchor_y" | "last_mods" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Reset the rotation to the base (the AI-first peer of a snap-to-zero).
            "reset" => {
                *self = RotationGizmo::new();
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The view ---------------------------------------------------------------

fn read_gizmo(scene: &Scene) -> ViewState {
    let Some(intro) = scene
        .find_external_with_tag(PANE_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return ViewState::default();
    };
    let angle_deg = match intro.query("angle_deg") {
        Some(IntrospectValue::Float(f)) => f,
        _ => 0.0,
    };
    let events = match intro.query("events") {
        Some(IntrospectValue::Int(n)) => u64::try_from(n).unwrap_or(0),
        _ => 0,
    };
    let phase = match intro.query("phase") {
        Some(IntrospectValue::Text(s)) => GesturePhase::from_wire_name(&s),
        _ => None,
    };
    let anchor = match (query_frac(intro, "anchor_x"), query_frac(intro, "anchor_y")) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    ViewState {
        angle_deg,
        phase,
        events,
        anchor,
    }
}

fn query_frac(intro: &dyn ExternalIntrospect, path: &str) -> Option<f32> {
    match intro.query(path) {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "an anchor fraction 0.0..=1.0 loses no meaningful precision as f32"
        )]
        Some(IntrospectValue::Float(f)) => Some(f as f32),
        _ => None,
    }
}

/// view-fn (§6.3): pure sync mapping of the gizmo digest to a scene.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the WidgetCore::view trait hands the frame by reference; the signature mirrors it"
)]
fn view(state: ViewState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);
    let pane_fill = theme.resolve(ColorRole::SurfaceContainerLow);
    let outline = theme.resolve(ColorRole::Outline);
    let accent = theme.resolve(ColorRole::Accent);

    let title = Scene::Text(
        TextNode::styled(
            "Twist to rotate — degrees accumulate, cancel reverts",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(20, 18)),
    );

    // The visible pane body, behind the transparent capture surface.
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

    // The fixed hub the needle orbits.
    let hub = hub_scene(outline);
    // The rotating needle tip: its rect orbits the hub with the accumulated
    // angle. A snapshot reads it, so the rotation is visible in the scene data,
    // not only in the introspect field.
    let needle = needle_scene(&state, accent, outline);

    // Transparent, pointer-opaque capture surface over the pane — the `gizmo`
    // primary tag the External registers under. On top so a twist anywhere over
    // the pane resolves to it; transparent so the needle shows through.
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
        .with_layout(LayoutStyle::new().with_absolute_position(20, WIN_H - 30)),
    );

    Scene::Container(
        ContainerNode::new(vec![pane_body, hub, needle, pane_surface, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// The fixed centre hub — a small square at the orbit centre, drawn under the
/// needle so the needle appears to pivot around it.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the hub centre is a fixed pane-interior pixel; the fractional part is not meaningful for a paint rect"
)]
fn hub_scene(outline: pinion_core::style::Color) -> Scene {
    const HUB_SIDE: u32 = 10;
    let x = (HUB_X - f64::from(HUB_SIDE) / 2.0).round() as u32;
    let y = (HUB_Y - f64::from(HUB_SIDE) / 2.0).round() as u32;
    Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(outline)).with_layout(
            LayoutStyle::new()
                .with_absolute_position(x, y)
                .with_size(Size::px(HUB_SIDE, HUB_SIDE)),
        ),
    )
}

/// The needle tip: a square at [`needle_rect`]'s window-absolute position for the
/// accumulated angle.
fn needle_scene(
    state: &ViewState,
    fill: pinion_core::style::Color,
    outline: pinion_core::style::Color,
) -> Scene {
    let rect = needle_rect(state.angle_deg);
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(fill).with_border(Border::new(outline, 2)),
        )
        .with_tag(NEEDLE_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h)),
        ),
    )
}

// --- The binding ------------------------------------------------------------

struct RotateGestureView;

impl WidgetCore for RotateGestureView {
    type State = ViewState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(RotationGizmo::new())
    }

    fn tag() -> &'static str {
        PANE_TAG
    }

    fn read_state(scene: &Scene) -> ViewState {
        read_gizmo(scene)
    }

    fn view(state: ViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-rotate-gesture (R1433 §5.35 native rotation-gesture spin)"
    }

    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &ViewState) -> String {
        readout_text(state)
    }
}

impl WidgetA11y for RotateGestureView {
    fn access_node(state: &ViewState, _focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(PANE_TAG, AriaRole::Group)
                .with_name("Rotation gizmo")
                .with_value(AccessValue::Text(readout_text(state))),
        ]
    }
}

impl WidgetView for RotateGestureView {
    type Renderer = HelloRotateGestureRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<RotateGestureView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eps(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn update_accumulates_rotation_additively() {
        let mut g = RotationGizmo::new();
        // Begin snapshots the base (delta 0), then a +45° Update rotates CCW.
        assert!(g.rotation_gesture(0.5, 0.5, 0.0, GesturePhase::Begin, Modifiers::empty()));
        assert!(g.rotation_gesture(0.5, 0.5, 45.0, GesturePhase::Update, Modifiers::empty()));
        assert!(eps(g.angle_deg, 45.0), "angle {}", g.angle_deg);
        // A second +45° Update adds (45 + 45 = 90), not multiplies.
        g.rotation_gesture(0.5, 0.5, 45.0, GesturePhase::Update, Modifiers::empty());
        assert!(eps(g.angle_deg, 90.0), "angle {}", g.angle_deg);
        // A negative delta rotates clockwise back.
        g.rotation_gesture(0.5, 0.5, -30.0, GesturePhase::Update, Modifiers::empty());
        assert!(eps(g.angle_deg, 60.0), "angle {}", g.angle_deg);
        assert_eq!(g.events, 4);
        assert_eq!(g.phase, Some(GesturePhase::Update));
    }

    #[test]
    fn cancel_reverts_the_whole_arc_but_end_commits() {
        let mut g = RotationGizmo::new();
        // Rotate to 90° and commit with End.
        g.rotation_gesture(0.5, 0.5, 0.0, GesturePhase::Begin, Modifiers::empty());
        g.rotation_gesture(0.5, 0.5, 90.0, GesturePhase::Update, Modifiers::empty());
        g.rotation_gesture(0.5, 0.5, 0.0, GesturePhase::End, Modifiers::empty());
        assert!(eps(g.angle_deg, 90.0), "committed angle {}", g.angle_deg);
        // A second arc that CANCELS reverts to the committed 90°, discarding the
        // in-flight rotation — the whole point of the phase bracket.
        g.rotation_gesture(0.5, 0.5, 0.0, GesturePhase::Begin, Modifiers::empty());
        g.rotation_gesture(0.5, 0.5, 90.0, GesturePhase::Update, Modifiers::empty());
        assert!(eps(g.angle_deg, 180.0), "mid-arc angle {}", g.angle_deg);
        g.rotation_gesture(0.5, 0.5, 0.0, GesturePhase::Cancel, Modifiers::empty());
        assert!(
            eps(g.angle_deg, 90.0),
            "cancel reverts to committed {}",
            g.angle_deg
        );
    }

    #[test]
    fn anchor_and_modifiers_are_recorded() {
        let mut g = RotationGizmo::new();
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        g.rotation_gesture(0.75, 0.25, 10.0, GesturePhase::Begin, shift);
        assert_eq!(g.anchor, Some((0.75, 0.25)));
        assert!(g.modifiers.shift_key(), "shift-snap bit recorded");
        // The introspect surface mirrors the fields (§2 #7).
        assert!(
            matches!(g.query("anchor_x"), Some(IntrospectValue::Float(f)) if (f - 0.75).abs() < 1e-6)
        );
        assert!(matches!(g.query("phase"), Some(IntrospectValue::Text(ref s)) if s == "begin"));
    }

    #[test]
    fn reset_returns_to_base() {
        let mut g = RotationGizmo::new();
        g.rotation_gesture(0.5, 0.5, 30.0, GesturePhase::Begin, Modifiers::empty());
        assert!(g.angle_deg > 0.0);
        g.invoke("reset", IntrospectValue::Null).unwrap();
        assert!(eps(g.angle_deg, 0.0));
        assert_eq!(g.events, 0);
        assert_eq!(g.phase, None);
    }

    /// The needle tip's window-absolute CENTRE for an accumulated angle (the
    /// rect anchors at the top-left, so add the half-side to compare against the
    /// hub centre).
    fn tip_center(angle_deg: f64) -> (u32, u32) {
        let r = needle_rect(angle_deg);
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    #[test]
    fn needle_tip_orbits_with_the_angle() {
        // At 0° the tip is to the RIGHT of the hub; at +90° (counter-clockwise)
        // it lifts to ABOVE the hub (smaller y, screen y grows downward). The
        // paint rect a snapshot reads moves with the accumulated angle.
        let (x0, y0) = tip_center(0.0);
        let (x90, y90) = tip_center(90.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let hub_x = HUB_X.round() as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let hub_y = HUB_Y.round() as u32;
        assert!(x0 > hub_x, "0deg tip is right of the hub: {x0} > {hub_x}");
        assert!(
            y0.abs_diff(hub_y) <= 1,
            "0deg tip is level with the hub: {y0} ~ {hub_y}"
        );
        assert!(
            y90 < y0,
            "+90deg tip lifts above the 0deg tip: {y90} < {y0}"
        );
        assert!(
            x90.abs_diff(hub_x) <= 1,
            "+90deg tip is centred over the hub in x: {x90} ~ {hub_x}"
        );
    }
}
