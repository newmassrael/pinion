//! R1432 §5.35 §5.15 — a native PINCH (magnify) gesture zooms a viewport.
//!
//! A `PinchViewport` [`External`] overrides [`External::pinch_gesture`] — the toolkit native gesture event `ZoomNativeGesture` / macOS
//! `magnify:` peer — and accumulates the INCREMENTAL magnification of a two-finger
//! trackpad pinch across the [`GesturePhase::Begin`]`..`[`End`](GesturePhase::End) arc: `scale *= 1.0 + magnification` per
//! event, clamped to a sane range. The whole point of the phase is the arc
//! bracket — on [`GesturePhase::Cancel`] the viewport DISCARDS the in-flight zoom and reverts to
//! the scale it held when the gesture began, exactly as a preview drops when
//! the fingers lift without committing.
//!
//! winit surfaces `WindowEvent::PinchGesture` only on macOS / iOS, so the
//! `scene/pinch_gesture` RPC is the AI-first driver (§2 #2): a headless client
//! zooms the viewport with no trackpad, and reads the live scale back through
//! the introspect surface (§2 #7) — the square's paint rect and the `scale`
//! field move together.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaField,
    ThreadOwnership,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{Border, BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, GesturePhase, Modifiers, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloPinchZoomRenderer, HelloPinchZoomRendererError);

const WIN_W: u32 = 640;
const WIN_H: u32 = 440;
const THEME_TAG: &str = "app";

/// The viewport's paint tag **and** the [`PinchViewport`]'s registration tag —
/// addressed over RPC as `/external/<field>`. A transparent, pointer-opaque
/// capture surface over the pane carries it, so a pinch anywhere over the pane
/// resolves to the viewport (the router offers a native gesture to the widget
/// under the cursor).
const PANE_TAG: &str = "viewport";

/// The zoom target square's paint tag. A snapshot reads its rect to confirm the
/// square grows / shrinks with the accumulated pinch scale.
const SQUARE_TAG: &str = "zoom.square";

/// The human-readable report line at the window's foot.
const READOUT_TAG: &str = "pinch.readout";

/// The zoomable square's side at `scale == 1.0` (px); the drawn side is
/// `BASE_SIDE_PX * scale`, clamped to the pane.
const BASE_SIDE_PX: f64 = 48.0;

/// The accumulated-scale clamp range: a pinch can shrink to a quarter or grow to
/// six times, no further (a native pinch never runs away — the OS clamps too).
const MIN_SCALE: f64 = 0.25;
const MAX_SCALE: f64 = 6.0;

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 13;

/// Window-absolute pane region. The transparent capture surface covers exactly
/// this rect, so a pinch's `x_rel` / `y_rel` fraction `0.0..=1.0` is the anchor
/// position on the viewport.
const PANE_RECT: Rect = Rect::new(20, 56, WIN_W - 40, WIN_H - 120);

// --- The paint-facing state snapshot ---------------------------------------

/// The read-only projection the binding paints, read back off the
/// [`PinchViewport`]'s introspect surface — the SAME fields the AI-first
/// `scene/query` client sees, so the picture and the data can never disagree.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ViewState {
    /// The accumulated zoom factor (`1.0` = the base size); drives the square.
    scale: f64,
    /// The last gesture phase, or `None` before the first pinch.
    phase: Option<GesturePhase>,
    /// The number of pinch events received (monotone until `reset`).
    events: u64,
    /// The anchor fraction of the last pinch (`None` before the first).
    anchor: Option<(f32, f32)>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            scale: 1.0,
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
        None => "pinch to zoom the square (scene/pinch_gesture drives it headless)".to_owned(),
        Some(phase) => {
            let anchor = match state.anchor {
                Some((x, y)) => format!(" @ ({x:.2}, {y:.2})"),
                None => String::new(),
            };
            format!(
                "scale {:.2}× — {} — {} event(s){anchor}",
                state.scale,
                phase.as_wire_name(),
                state.events,
            )
        }
    }
}

// --- The pinch viewport (primary External) ---------------------------------

/// The zoom authority. Overrides [`External::pinch_gesture`] and accumulates the
/// incremental magnification across the gesture arc, reverting on cancel.
#[derive(Debug)]
struct PinchViewport {
    /// The accumulated zoom factor, clamped `MIN_SCALE..=MAX_SCALE`.
    scale: f64,
    /// The scale captured at the last [`GesturePhase::Begin`], restored on a
    /// [`GesturePhase::Cancel`] so an aborted pinch discards its whole arc.
    scale_at_begin: f64,
    /// The last incremental magnification delta (the raw `pinch_gesture` value).
    magnification: f64,
    /// The last gesture phase, or `None` before the first pinch.
    phase: Option<GesturePhase>,
    /// The number of pinch events received (monotone until `reset`).
    events: u64,
    /// The anchor fraction of the last pinch (`None` before the first).
    anchor: Option<(f32, f32)>,
    /// The modifiers held on the last pinch (the toolkit-parity `Ctrl`-fine-zoom
    /// bit).
    modifiers: Modifiers,
}

impl Default for PinchViewport {
    fn default() -> Self {
        Self {
            scale: 1.0,
            scale_at_begin: 1.0,
            magnification: 0.0,
            phase: None,
            events: 0,
            anchor: None,
            modifiers: Modifiers::empty(),
        }
    }
}

impl PinchViewport {
    fn new() -> Self {
        Self::default()
    }

    /// Fold one incremental magnification delta into the accumulated scale,
    /// clamped to the sane range: `scale *= 1.0 + magnification`.
    fn apply_zoom(&mut self, magnification: f64) {
        self.scale = (self.scale * (1.0 + magnification)).clamp(MIN_SCALE, MAX_SCALE);
    }
}

impl External for PinchViewport {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// R1432 — the seam under test. Accumulate the incremental magnification
    /// across the `Begin..End` arc; snapshot the scale on `Begin` so `Cancel`
    /// can revert the whole arc; commit (keep the scale) on `End`. Always
    /// consumes — the viewport owns the pinch (a native gesture has no scroll
    /// fallback to decline to).
    fn pinch_gesture(
        &mut self,
        x_rel: f32,
        y_rel: f32,
        magnification: f64,
        phase: GesturePhase,
        modifiers: Modifiers,
    ) -> bool {
        self.events += 1;
        self.anchor = Some((x_rel.clamp(0.0, 1.0), y_rel.clamp(0.0, 1.0)));
        self.phase = Some(phase);
        self.magnification = magnification;
        self.modifiers = modifiers;
        match phase {
            GesturePhase::Begin => {
                self.scale_at_begin = self.scale;
                self.apply_zoom(magnification);
            }
            GesturePhase::Update => self.apply_zoom(magnification),
            GesturePhase::End => {}
            GesturePhase::Cancel => self.scale = self.scale_at_begin,
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

impl ExternalIntrospect for PinchViewport {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The accumulated zoom factor (1.0 = base); drives the square.
                    SchemaField::new("scale", "float"),
                    // The scale captured at the last Begin (Cancel reverts to it).
                    SchemaField::new("scale_at_begin", "float"),
                    // The last incremental magnification delta.
                    SchemaField::new("magnification", "float"),
                    // The last GesturePhase: begin / update / end / cancel (Null yet).
                    SchemaField::new("phase", "string"),
                    // The pinch event count (monotone until `reset`).
                    SchemaField::new("events", "int"),
                    // The anchor fraction of the last pinch (Null before the first).
                    SchemaField::new("anchor_x", "float"),
                    SchemaField::new("anchor_y", "float"),
                    // The modifiers held at the last pinch, as a wire token (e.g. "c").
                    SchemaField::new("last_mods", "string"),
                    // Reset the zoom — the AI-first peer of a double-tap fit.
                    SchemaField::action("reset", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "scale" => Ok(IntrospectValue::Float(self.scale)),
            "scale_at_begin" => Ok(IntrospectValue::Float(self.scale_at_begin)),
            "magnification" => Ok(IntrospectValue::Float(self.magnification)),
            "phase" => Ok(self.phase.map_or(IntrospectValue::Null, |p| {
                IntrospectValue::Text(p.as_wire_name().to_owned())
            })),
            "events" => Ok(IntrospectValue::Int(
                i64::try_from(self.events).unwrap_or(i64::MAX),
            )),
            "anchor_x" => Ok(self.anchor.map_or(IntrospectValue::Null, |(x, _)| {
                IntrospectValue::Float(x.into())
            })),
            "anchor_y" => Ok(self.anchor.map_or(IntrospectValue::Null, |(_, y)| {
                IntrospectValue::Float(y.into())
            })),
            "last_mods" => Ok(IntrospectValue::Text(self.modifiers.as_wire_token())),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Every field is a read-only projection of the gesture stream.
            "scale" | "scale_at_begin" | "magnification" | "phase" | "events" | "anchor_x"
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
            // Reset the zoom to the base (the AI-first peer of a fit-to-view).
            "reset" => {
                *self = PinchViewport::new();
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The view ---------------------------------------------------------------

fn read_viewport(scene: &Scene) -> ViewState {
    let Some(intro) = scene
        .find_external_with_tag(PANE_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return ViewState::default();
    };
    let scale = match intro.query("scale") {
        Ok(IntrospectValue::Float(f)) => f,
        _ => 1.0,
    };
    let events = match intro.query("events") {
        Ok(IntrospectValue::Int(n)) => u64::try_from(n).unwrap_or(0),
        _ => 0,
    };
    let phase = match intro.query("phase") {
        Ok(IntrospectValue::Text(s)) => GesturePhase::from_wire_name(&s),
        _ => None,
    };
    let anchor = match (query_frac(intro, "anchor_x"), query_frac(intro, "anchor_y")) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    ViewState {
        scale,
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
        Ok(IntrospectValue::Float(f)) => Some(f as f32),
        _ => None,
    }
}

/// view-fn (§6.3): pure sync mapping of the viewport digest to a scene.
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
            "Pinch to zoom — magnify accumulates, cancel reverts",
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

    // The zoom target: a square centred in the pane whose side is the accumulated
    // scale × the base side. A snapshot reads its rect, so the zoom is visible in
    // the scene data, not only in the introspect field.
    let square = zoom_square_scene(&state, accent, outline);

    // Transparent, pointer-opaque capture surface over the pane — the `viewport`
    // primary tag the External registers under. On top so a pinch anywhere over
    // the pane resolves to it; transparent so the square shows through.
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
        ContainerNode::new(vec![pane_body, square, pane_surface, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// The zoom square: side = `BASE_SIDE_PX * scale`, clamped to the pane, centred
/// in it. The paint rect is what a `scene/snapshot` reads to confirm the pinch
/// zoomed the viewport.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the side is a small non-negative pixel count clamped to the pane; the fractional part is not meaningful for a paint rect"
)]
fn zoom_square_scene(
    state: &ViewState,
    fill: pinion_core::style::Color,
    outline: pinion_core::style::Color,
) -> Scene {
    let max_side = PANE_RECT.w.min(PANE_RECT.h);
    let side = (BASE_SIDE_PX * state.scale)
        .round()
        .clamp(1.0, f64::from(max_side)) as u32;
    let x = PANE_RECT.x + (PANE_RECT.w - side) / 2;
    let y = PANE_RECT.y + (PANE_RECT.h - side) / 2;
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(fill).with_border(Border::new(outline, 2)),
        )
        .with_tag(SQUARE_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(x, y)
                .with_size(Size::px(side, side)),
        ),
    )
}

// --- The binding ------------------------------------------------------------

struct PinchZoomView;

impl WidgetCore for PinchZoomView {
    type State = ViewState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(PinchViewport::new())
    }

    fn tag() -> &'static str {
        PANE_TAG
    }

    fn read_state(scene: &Scene) -> ViewState {
        read_viewport(scene)
    }

    fn view(state: ViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-pinch-zoom (R1432 §5.35 native pinch-gesture zoom)"
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

impl WidgetA11y for PinchZoomView {
    fn access_node(state: &ViewState, _focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(PANE_TAG, AriaRole::Group)
                .with_name("Pinch-zoom viewport")
                .with_value(AccessValue::Text(readout_text(state))),
        ]
    }
}

impl WidgetView for PinchZoomView {
    type Renderer = HelloPinchZoomRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<PinchZoomView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eps(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn update_accumulates_magnification_and_clamps() {
        let mut v = PinchViewport::new();
        // Begin snapshots the base, then a +50% Update zooms in.
        assert!(v.pinch_gesture(0.5, 0.5, 0.0, GesturePhase::Begin, Modifiers::empty()));
        assert!(v.pinch_gesture(0.5, 0.5, 0.5, GesturePhase::Update, Modifiers::empty()));
        assert!(eps(v.scale, 1.5), "scale {}", v.scale);
        // A second +100% Update compounds multiplicatively (1.5 * 2.0 = 3.0).
        v.pinch_gesture(0.5, 0.5, 1.0, GesturePhase::Update, Modifiers::empty());
        assert!(eps(v.scale, 3.0), "scale {}", v.scale);
        // A huge delta clamps at MAX_SCALE, never runs away.
        v.pinch_gesture(0.5, 0.5, 10.0, GesturePhase::Update, Modifiers::empty());
        assert!(eps(v.scale, MAX_SCALE), "scale {}", v.scale);
        assert_eq!(v.events, 4);
        assert_eq!(v.phase, Some(GesturePhase::Update));
    }

    #[test]
    fn cancel_reverts_the_whole_arc_but_end_commits() {
        let mut v = PinchViewport::new();
        // Zoom to 2× and commit with End.
        v.pinch_gesture(0.5, 0.5, 0.0, GesturePhase::Begin, Modifiers::empty());
        v.pinch_gesture(0.5, 0.5, 1.0, GesturePhase::Update, Modifiers::empty());
        v.pinch_gesture(0.5, 0.5, 0.0, GesturePhase::End, Modifiers::empty());
        assert!(eps(v.scale, 2.0), "committed scale {}", v.scale);
        // A second arc that CANCELS reverts to the committed 2×, discarding the
        // in-flight zoom — the whole point of the phase bracket.
        v.pinch_gesture(0.5, 0.5, 0.0, GesturePhase::Begin, Modifiers::empty());
        v.pinch_gesture(0.5, 0.5, 1.0, GesturePhase::Update, Modifiers::empty());
        assert!(eps(v.scale, 4.0), "mid-arc scale {}", v.scale);
        v.pinch_gesture(0.5, 0.5, 0.0, GesturePhase::Cancel, Modifiers::empty());
        assert!(eps(v.scale, 2.0), "cancel reverts to committed {}", v.scale);
    }

    #[test]
    fn anchor_and_modifiers_are_recorded() {
        let mut v = PinchViewport::new();
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::empty()
        };
        v.pinch_gesture(0.75, 0.25, 0.1, GesturePhase::Begin, ctrl);
        assert_eq!(v.anchor, Some((0.75, 0.25)));
        assert!(v.modifiers.control_key(), "ctrl-fine-zoom bit recorded");
        // The introspect surface mirrors the fields (§2 #7).
        assert!(
            matches!(v.query("anchor_x"), Ok(IntrospectValue::Float(f)) if (f - 0.75).abs() < 1e-6)
        );
        assert!(matches!(v.query("phase"), Ok(IntrospectValue::Text(ref s)) if s == "begin"));
    }

    #[test]
    fn reset_returns_to_base() {
        let mut v = PinchViewport::new();
        v.pinch_gesture(0.5, 0.5, 1.0, GesturePhase::Begin, Modifiers::empty());
        assert!(v.scale > 1.0);
        v.invoke("reset", IntrospectValue::Null).unwrap();
        assert!(eps(v.scale, 1.0));
        assert_eq!(v.events, 0);
        assert_eq!(v.phase, None);
    }

    fn square_side_px(state: &ViewState) -> u32 {
        use pinion_core::style::{Color, SizeValue};
        let Scene::Box(ref b) = zoom_square_scene(state, Color::default(), Color::default()) else {
            panic!("expected Box");
        };
        match b.layout.size.width {
            SizeValue::Px(w) => w,
            other => panic!("expected a px width, got {other:?}"),
        }
    }

    #[test]
    fn square_side_tracks_scale() {
        // The paint rect a snapshot reads grows with the accumulated scale.
        let base_side = square_side_px(&ViewState::default());
        let zoomed_side = square_side_px(&ViewState {
            scale: 2.0,
            ..ViewState::default()
        });
        assert!(
            zoomed_side > base_side,
            "2x scale must enlarge the square: {base_side} -> {zoomed_side}"
        );
    }
}
