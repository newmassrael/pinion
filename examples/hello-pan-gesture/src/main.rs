//! R1434 §5.35 §5.15 — a native PAN gesture slides a map.
//!
//! A `MapViewport` [`External`] overrides [`External::pan_gesture`] — the toolkit native gesture event `PanNativeGesture` / winit
//! `WindowEvent::PanGesture` peer — and accumulates the INCREMENTAL two-axis delta of an N-finger
//! trackpad pan across the [`GesturePhase::Begin`]`..`[`End`](GesturePhase::End) arc: `offset += delta` per
//! event, in logical pixels, with the platform's own sign (a native pan is
//! direct manipulation — the content follows the fingers — so it is NOT
//! sign-flipped the way a wheel scroll command is). The offset is clamped to
//! the content bounds, the way a real map stops at the edge of its tiles, and
//! — the whole point of the phase — on [`GesturePhase::Cancel`] the viewport DISCARDS the
//! in-flight pan and snaps back to the offset it held when the gesture began.
//!
//! winit surfaces `WindowEvent::PanGesture` only on iOS, so the
//! `scene/pan_gesture` RPC is the AI-first driver (§2 #2): a headless client
//! slides the map with no trackpad, and reads the live offset back through the
//! introspect surface (§2 #7) — the marker's paint rect and the `offset_x` /
//! `offset_y` fields move together.

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
vello_renderer_impl!(HelloPanGestureRenderer, HelloPanGestureRendererError);

const WIN_W: u32 = 640;
const WIN_H: u32 = 440;
const THEME_TAG: &str = "app";

/// The viewport's paint tag **and** the [`MapViewport`]'s registration tag —
/// addressed over RPC as `/external/<field>`. A transparent, pointer-opaque
/// capture surface over the pane carries it, so a pan anywhere over the pane
/// resolves to the viewport (the router offers a native gesture to the widget
/// under the cursor).
const PANE_TAG: &str = "map";

/// The map marker's paint tag — the content pin a snapshot reads to confirm the
/// content translated with the pan, not only the introspect field.
const MARKER_TAG: &str = "pan.marker";

/// The human-readable report line at the window's foot.
const READOUT_TAG: &str = "pan.readout";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 13;

/// Window-absolute pane region. The transparent capture surface covers exactly
/// this rect, so a pan's `x_rel` / `y_rel` fraction `0.0..=1.0` is the anchor
/// position on the map.
const PANE_RECT: Rect = Rect::new(20, 56, WIN_W - 40, WIN_H - 120);

/// The map's rest position — the marker's centre at offset `(0, 0)`.
const HOME_X: f64 = PANE_RECT.x as f64 + PANE_RECT.w as f64 / 2.0;
const HOME_Y: f64 = PANE_RECT.y as f64 + PANE_RECT.h as f64 / 2.0;

/// The content bound: the offset saturates at ±[`MAX_PAN`] on each axis, the way
/// a map stops at the edge of its tiles. Every accumulated offset the widget
/// reports is already clamped, so the paint and the field can never disagree.
const MAX_PAN: f64 = 120.0;

/// The marker's side and the satellite pin's side + rest offset from the marker.
/// The satellite exists so the pan reads as a *content translation* (two things
/// move together, keeping their relative geometry), not a lone dot drifting.
const MARKER_SIDE: u32 = 20;
const SATELLITE_SIDE: u32 = 12;
const SATELLITE_DX: f64 = 70.0;
const SATELLITE_DY: f64 = -45.0;

// --- The paint-facing state snapshot ---------------------------------------

/// The read-only projection the binding paints, read back off the
/// [`MapViewport`]'s introspect surface — the SAME fields the AI-first
/// `scene/query` client sees, so the picture and the data can never disagree.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct ViewState {
    /// The accumulated pan offset in logical pixels (`(0, 0)` = home).
    offset: (f64, f64),
    /// The last gesture phase, or `None` before the first pan.
    phase: Option<GesturePhase>,
    /// The number of pan events received (monotone until `reset`).
    events: u64,
    /// The anchor fraction of the last pan (`None` before the first).
    anchor: Option<(f32, f32)>,
}

/// The idle prompt / live report line — the SSOT both the status text and the
/// a11y value read.
fn readout_text(state: &ViewState) -> String {
    match state.phase {
        None => "two-finger pan to slide the map (scene/pan_gesture drives it headless)".to_owned(),
        Some(phase) => {
            let anchor = match state.anchor {
                Some((x, y)) => format!(" @ ({x:.2}, {y:.2})"),
                None => String::new(),
            };
            format!(
                "offset ({:.0}, {:.0}) — {} — {} event(s){anchor}",
                state.offset.0,
                state.offset.1,
                phase.as_wire_name(),
                state.events,
            )
        }
    }
}

/// A content box's window-absolute rect for the accumulated `offset`: the whole
/// map translates rigidly, so every pin shares one formula (`home + offset`) and
/// keeps its relative geometry through the pan. These paint rects are what a
/// `scene/snapshot` reads to confirm the gesture slid the content.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a content pin stays inside the window (home ± MAX_PAN plus a fixed inset), a small non-negative pixel count; the fractional part is not meaningful for a paint rect"
)]
fn content_rect(home: (f64, f64), offset: (f64, f64), side: u32) -> Rect {
    let half = f64::from(side) / 2.0;
    let x = (home.0 + offset.0 - half)
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32;
    let y = (home.1 + offset.1 - half)
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32;
    Rect::new(x, y, side, side)
}

// --- The map viewport (primary External) ------------------------------------

/// The pan authority. Overrides [`External::pan_gesture`] and accumulates the
/// incremental two-axis delta across the gesture arc, clamped to the content
/// bounds and reverted on cancel.
#[derive(Debug, Default)]
struct MapViewport {
    /// The accumulated offset in logical pixels, always within ±[`MAX_PAN`].
    offset: (f64, f64),
    /// The offset captured at the last [`GesturePhase::Begin`], restored on a
    /// [`GesturePhase::Cancel`] so an aborted pan discards its whole arc.
    offset_at_begin: (f64, f64),
    /// The last incremental delta (the raw `pan_gesture` values).
    delta: (f32, f32),
    /// The last gesture phase, or `None` before the first pan.
    phase: Option<GesturePhase>,
    /// The number of pan events received (monotone until `reset`).
    events: u64,
    /// The anchor fraction of the last pan (`None` before the first).
    anchor: Option<(f32, f32)>,
    /// The modifiers held on the last pan (the toolkit-parity axis-lock bit).
    modifiers: Modifiers,
}

impl MapViewport {
    fn new() -> Self {
        Self::default()
    }

    /// Fold one incremental delta into the accumulated offset, saturating at the
    /// content bound: `offset = clamp(offset + delta, ±MAX_PAN)`. Clamping here
    /// (not at paint) keeps the introspect field and the marker rect the same
    /// number — an AI reading `offset_x` learns where the map actually is, not
    /// where an unclamped accumulator wishes it were.
    fn apply_delta(&mut self, delta: (f32, f32)) {
        self.offset = (
            (self.offset.0 + f64::from(delta.0)).clamp(-MAX_PAN, MAX_PAN),
            (self.offset.1 + f64::from(delta.1)).clamp(-MAX_PAN, MAX_PAN),
        );
    }
}

impl External for MapViewport {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// R1434 — the seam under test. Accumulate the incremental delta across the
    /// `Begin..End` arc; snapshot the offset on `Begin` so `Cancel` can revert
    /// the whole arc; commit (keep the offset) on `End`. Always consumes — the
    /// viewport owns the pan (a native gesture has no fallback to decline to).
    fn pan_gesture(
        &mut self,
        x_rel: f32,
        y_rel: f32,
        delta_x: f32,
        delta_y: f32,
        phase: GesturePhase,
        modifiers: Modifiers,
    ) -> bool {
        self.events += 1;
        self.anchor = Some((x_rel.clamp(0.0, 1.0), y_rel.clamp(0.0, 1.0)));
        self.phase = Some(phase);
        self.delta = (delta_x, delta_y);
        self.modifiers = modifiers;
        match phase {
            GesturePhase::Begin => {
                self.offset_at_begin = self.offset;
                self.apply_delta((delta_x, delta_y));
            }
            GesturePhase::Update => self.apply_delta((delta_x, delta_y)),
            GesturePhase::End => {}
            GesturePhase::Cancel => self.offset = self.offset_at_begin,
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

impl ExternalIntrospect for MapViewport {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The accumulated offset in logical px (0 = home); drives the map.
                    SchemaField::new("offset_x", "float"),
                    SchemaField::new("offset_y", "float"),
                    // The offset captured at the last Begin (Cancel reverts to it).
                    SchemaField::new("offset_at_begin_x", "float"),
                    SchemaField::new("offset_at_begin_y", "float"),
                    // The last incremental delta (logical px, both axes).
                    SchemaField::new("delta_x", "float"),
                    SchemaField::new("delta_y", "float"),
                    // The last GesturePhase: begin / update / end / cancel (Null yet).
                    SchemaField::new("phase", "string"),
                    // The pan event count (monotone until `reset`).
                    SchemaField::new("events", "int"),
                    // The anchor fraction of the last pan (Null before the first).
                    SchemaField::new("anchor_x", "float"),
                    SchemaField::new("anchor_y", "float"),
                    // The modifiers held at the last pan, as a wire token (e.g. "s").
                    SchemaField::new("last_mods", "string"),
                    // Recentre the map — the AI-first peer of a snap-to-home.
                    SchemaField::action("reset", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "offset_x" => Some(IntrospectValue::Float(self.offset.0)),
            "offset_y" => Some(IntrospectValue::Float(self.offset.1)),
            "offset_at_begin_x" => Some(IntrospectValue::Float(self.offset_at_begin.0)),
            "offset_at_begin_y" => Some(IntrospectValue::Float(self.offset_at_begin.1)),
            "delta_x" => Some(IntrospectValue::Float(self.delta.0.into())),
            "delta_y" => Some(IntrospectValue::Float(self.delta.1.into())),
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
            "offset_x" | "offset_y" | "offset_at_begin_x" | "offset_at_begin_y" | "delta_x"
            | "delta_y" | "phase" | "events" | "anchor_x" | "anchor_y" | "last_mods" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Recentre the map (the AI-first peer of a snap-to-home).
            "reset" => {
                *self = MapViewport::new();
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
    let offset = (query_f64(intro, "offset_x"), query_f64(intro, "offset_y"));
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
        offset,
        phase,
        events,
        anchor,
    }
}

fn query_f64(intro: &dyn ExternalIntrospect, path: &str) -> f64 {
    match intro.query(path) {
        Some(IntrospectValue::Float(f)) => f,
        _ => 0.0,
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
            "Two-finger pan to slide the map — the content follows the fingers",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(20, 18)),
    );

    // The visible pane body — the viewport frame the content slides behind.
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

    // The map content: the marker and its satellite translate rigidly with the
    // accumulated offset, so a snapshot sees the pan, not only the field.
    let marker = content_box(
        content_rect((HOME_X, HOME_Y), state.offset, MARKER_SIDE),
        Some(MARKER_TAG),
        accent,
        outline,
    );
    let satellite = content_box(
        content_rect(
            (HOME_X + SATELLITE_DX, HOME_Y + SATELLITE_DY),
            state.offset,
            SATELLITE_SIDE,
        ),
        None,
        outline,
        outline,
    );

    // Transparent, pointer-opaque capture surface over the pane — the `map`
    // primary tag the External registers under. On top so a pan anywhere over
    // the pane resolves to it; transparent so the content shows through.
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
        ContainerNode::new(vec![
            pane_body,
            satellite,
            marker,
            pane_surface,
            title,
            status,
        ])
        .with_style(BoxStyle::filled(surface))
        .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// One map-content box at a window-absolute rect, tagged when a snapshot needs
/// to address it.
fn content_box(
    rect: Rect,
    tag: Option<&'static str>,
    fill: pinion_core::style::Color,
    outline: pinion_core::style::Color,
) -> Scene {
    let node = BoxNode::new(
        Rect::default(),
        BoxStyle::filled(fill).with_border(Border::new(outline, 2)),
    )
    .with_layout(
        LayoutStyle::new()
            .with_absolute_position(rect.x, rect.y)
            .with_size(Size::px(rect.w, rect.h)),
    );
    Scene::Box(match tag {
        Some(t) => node.with_tag(t),
        None => node,
    })
}

// --- The binding ------------------------------------------------------------

struct PanGestureView;

impl WidgetCore for PanGestureView {
    type State = ViewState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(MapViewport::new())
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
        "pinion hello-pan-gesture (R1434 §5.35 native pan-gesture slide)"
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

impl WidgetA11y for PanGestureView {
    fn access_node(state: &ViewState, _focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(PANE_TAG, AriaRole::Group)
                .with_name("Map viewport")
                .with_value(AccessValue::Text(readout_text(state))),
        ]
    }
}

impl WidgetView for PanGestureView {
    type Renderer = HelloPanGestureRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<PanGestureView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eps(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn update_accumulates_both_axes_additively() {
        let mut v = MapViewport::new();
        // Begin snapshots home (delta 0), then an Update slides right + down.
        assert!(v.pan_gesture(0.5, 0.5, 0.0, 0.0, GesturePhase::Begin, Modifiers::empty()));
        assert!(v.pan_gesture(
            0.5,
            0.5,
            30.0,
            20.0,
            GesturePhase::Update,
            Modifiers::empty()
        ));
        assert!(
            eps(v.offset.0, 30.0) && eps(v.offset.1, 20.0),
            "{:?}",
            v.offset
        );
        // A second Update ADDS on each axis independently, and a negative delta
        // slides back — the two axes never bleed into each other.
        v.pan_gesture(
            0.5,
            0.5,
            30.0,
            -50.0,
            GesturePhase::Update,
            Modifiers::empty(),
        );
        assert!(
            eps(v.offset.0, 60.0) && eps(v.offset.1, -30.0),
            "{:?}",
            v.offset
        );
        assert_eq!(v.events, 3);
        assert_eq!(v.phase, Some(GesturePhase::Update));
    }

    #[test]
    fn offset_saturates_at_the_content_bound() {
        let mut v = MapViewport::new();
        v.pan_gesture(0.5, 0.5, 0.0, 0.0, GesturePhase::Begin, Modifiers::empty());
        // A pan far past the tiles' edge stops AT the bound, not beyond it, and
        // the clamped value is what the introspect field reports.
        v.pan_gesture(
            0.5,
            0.5,
            900.0,
            900.0,
            GesturePhase::Update,
            Modifiers::empty(),
        );
        assert!(
            eps(v.offset.0, MAX_PAN) && eps(v.offset.1, MAX_PAN),
            "{:?}",
            v.offset
        );
        // The clamp is symmetric, and a saturated axis still responds to a
        // reversing delta immediately (no accumulated overshoot to unwind).
        v.pan_gesture(
            0.5,
            0.5,
            -1000.0,
            -1000.0,
            GesturePhase::Update,
            Modifiers::empty(),
        );
        assert!(
            eps(v.offset.0, -MAX_PAN) && eps(v.offset.1, -MAX_PAN),
            "{:?}",
            v.offset
        );
        v.pan_gesture(
            0.5,
            0.5,
            40.0,
            0.0,
            GesturePhase::Update,
            Modifiers::empty(),
        );
        assert!(eps(v.offset.0, -MAX_PAN + 40.0), "{:?}", v.offset);
    }

    #[test]
    fn cancel_reverts_the_whole_arc_but_end_commits() {
        let mut v = MapViewport::new();
        // Slide to (60, 40) and commit with End.
        v.pan_gesture(0.5, 0.5, 0.0, 0.0, GesturePhase::Begin, Modifiers::empty());
        v.pan_gesture(
            0.5,
            0.5,
            60.0,
            40.0,
            GesturePhase::Update,
            Modifiers::empty(),
        );
        v.pan_gesture(0.5, 0.5, 0.0, 0.0, GesturePhase::End, Modifiers::empty());
        assert!(
            eps(v.offset.0, 60.0) && eps(v.offset.1, 40.0),
            "committed {:?}",
            v.offset
        );
        // A second arc that CANCELS reverts to the committed offset, discarding
        // the in-flight pan — the whole point of the phase bracket.
        v.pan_gesture(0.5, 0.5, 0.0, 0.0, GesturePhase::Begin, Modifiers::empty());
        v.pan_gesture(
            0.5,
            0.5,
            -100.0,
            -90.0,
            GesturePhase::Update,
            Modifiers::empty(),
        );
        assert!(
            eps(v.offset.0, -40.0) && eps(v.offset.1, -50.0),
            "mid-arc {:?}",
            v.offset
        );
        v.pan_gesture(0.5, 0.5, 0.0, 0.0, GesturePhase::Cancel, Modifiers::empty());
        assert!(
            eps(v.offset.0, 60.0) && eps(v.offset.1, 40.0),
            "cancel reverts to committed {:?}",
            v.offset
        );
    }

    #[test]
    fn anchor_and_modifiers_are_recorded() {
        let mut v = MapViewport::new();
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        v.pan_gesture(0.75, 0.25, 10.0, 0.0, GesturePhase::Begin, shift);
        assert_eq!(v.anchor, Some((0.75, 0.25)));
        assert!(v.modifiers.shift_key(), "axis-lock bit recorded");
        // The introspect surface mirrors the fields (§2 #7).
        assert!(
            matches!(v.query("anchor_x"), Some(IntrospectValue::Float(f)) if (f - 0.75).abs() < 1e-6)
        );
        assert!(matches!(v.query("phase"), Some(IntrospectValue::Text(ref s)) if s == "begin"));
    }

    #[test]
    fn reset_recentres_the_map() {
        let mut v = MapViewport::new();
        v.pan_gesture(
            0.5,
            0.5,
            30.0,
            30.0,
            GesturePhase::Begin,
            Modifiers::empty(),
        );
        assert!(v.offset.0 > 0.0);
        v.invoke("reset", IntrospectValue::Null).unwrap();
        assert!(eps(v.offset.0, 0.0) && eps(v.offset.1, 0.0));
        assert_eq!(v.events, 0);
        assert_eq!(v.phase, None);
    }

    #[test]
    fn content_translates_rigidly_with_the_offset() {
        // Both pins move by the SAME delta and keep their relative geometry —
        // the map slides as one piece, which is what a snapshot verifies.
        let marker0 = content_rect((HOME_X, HOME_Y), (0.0, 0.0), MARKER_SIDE);
        let sat0 = content_rect(
            (HOME_X + SATELLITE_DX, HOME_Y + SATELLITE_DY),
            (0.0, 0.0),
            SATELLITE_SIDE,
        );
        let marker1 = content_rect((HOME_X, HOME_Y), (50.0, -25.0), MARKER_SIDE);
        let sat1 = content_rect(
            (HOME_X + SATELLITE_DX, HOME_Y + SATELLITE_DY),
            (50.0, -25.0),
            SATELLITE_SIDE,
        );
        assert_eq!(
            marker1.x,
            marker0.x + 50,
            "marker slides right by the delta"
        );
        assert_eq!(marker1.y + 25, marker0.y, "marker slides up by the delta");
        assert_eq!(
            sat1.x - sat0.x,
            marker1.x - marker0.x,
            "satellite shares the x delta"
        );
        assert_eq!(sat1.y, sat0.y - 25, "satellite shares the y delta");
    }
}
