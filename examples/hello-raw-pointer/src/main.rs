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
//! stamped at the last edge), `x_frac` / `y_frac` (the live hover position), and
//! `log` (the full `;`-joined sequence). Every button edge is driven no-pixel
//! via the new `scene/pointer_button` RPC method — the single-edge peer the
//! press-pair `scene/click` never expressed. See
//! `tools/demos/r1416_raw_pointer.py`.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{
    Frame, Modifiers, PointerButton, PointerEdge, RawPointerButton, Scene, WidgetCore,
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
    x_frac: Option<f32>,
    y_frac: Option<f32>,
}

impl ButtonReport {
    /// The compact `"<button>:<edge>:<mods>"` label — the same shape the router
    /// unit tests assert, so the wire and the tests read one vocabulary.
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
            format!("#{} {}{}", state.count, r.label(), pos)
        }
    }
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

    // Transparent, pointer-opaque surface over the pane — the `pane` primary
    // tag the sink registers under. On top so a button anywhere over the pane
    // resolves to it; transparent so the body shows through, pointer-opaque so
    // the hit-test lands here (geometric hit-test is alpha-independent).
    let pane_surface = Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
            .with_tag(PANE_TAG)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(PANE_RECT.x, PANE_RECT.y)
                    .with_size(Size::px(PANE_RECT.w, PANE_RECT.h)),
            ),
    );

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

    Scene::Container(
        ContainerNode::new(vec![pane_body, pane_surface, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
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
        Some(IntrospectValue::Int(n)) => usize::try_from(n).unwrap_or(0),
        _ => 0,
    };
    let last = read_last_report(intro);
    SinkState {
        count,
        last,
        x_frac: query_frac(intro, "x_frac"),
        y_frac: query_frac(intro, "y_frac"),
    }
}

/// Reassemble the last [`ButtonReport`] from the external's scalar query
/// surface — the same fields the AI-first `scene/query` client reads.
fn read_last_report(intro: &dyn ExternalIntrospect) -> Option<ButtonReport> {
    let button = match intro.query("last_button") {
        Some(IntrospectValue::Text(s)) => PointerButton::from_wire_name(&s)?,
        _ => return None,
    };
    let edge = match intro.query("last_edge") {
        Some(IntrospectValue::Text(s)) => PointerEdge::from_wire_name(&s)?,
        _ => return None,
    };
    let modifiers = match intro.query("last_mods") {
        Some(IntrospectValue::Text(s)) => Modifiers::from_wire_token(&s).unwrap_or_default(),
        _ => Modifiers::empty(),
    };
    Some(ButtonReport {
        button,
        edge,
        modifiers,
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
        Some(IntrospectValue::Float(f)) => Some(f as f32),
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

    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        self.x_frac = Some(x_rel.clamp(0.0, 1.0));
        self.y_frac = Some(y_rel.clamp(0.0, 1.0));
    }

    /// Record one raw button edge with the modifiers held at that edge and the
    /// live position stamped in. This is the seam under test.
    fn raw_pointer_button(&mut self, event: RawPointerButton) {
        self.reports.push(ButtonReport {
            button: event.button,
            edge: event.edge,
            modifiers: event.modifiers,
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
                    // The position fraction stamped at the last edge (Null off-pane).
                    SchemaField::new("last_x", "float"),
                    SchemaField::new("last_y", "float"),
                    // The live hover position fraction (Null off the pane).
                    SchemaField::new("x_frac", "float"),
                    SchemaField::new("y_frac", "float"),
                    // The full ";"-joined report sequence (empty string if none).
                    SchemaField::new("log", "string"),
                    // The router pointer boundary (Leave / Cancel clear the live
                    // position when the pointer leaves the pane).
                    SchemaField::new("send", "string"),
                    // Reset the log — the AI-first peer of clearing the pane.
                    SchemaField::new("clear", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let last = self.last();
        match path {
            "report_count" => Some(IntrospectValue::Int(
                i64::try_from(self.reports.len()).unwrap_or(i64::MAX),
            )),
            "last" => {
                Some(last.map_or(IntrospectValue::Null, |r| IntrospectValue::Text(r.label())))
            }
            "last_button" => Some(last.map_or(IntrospectValue::Null, |r| {
                IntrospectValue::Text(r.button.as_wire_name().to_owned())
            })),
            "last_edge" => Some(last.map_or(IntrospectValue::Null, |r| {
                IntrospectValue::Text(r.edge.as_wire_name().to_owned())
            })),
            "last_mods" => Some(last.map_or(IntrospectValue::Null, |r| {
                IntrospectValue::Text(r.modifiers.as_wire_token())
            })),
            "last_x" => Some(
                last.and_then(|r| r.x_frac)
                    .map_or(IntrospectValue::Null, |f| IntrospectValue::Float(f.into())),
            ),
            "last_y" => Some(
                last.and_then(|r| r.y_frac)
                    .map_or(IntrospectValue::Null, |f| IntrospectValue::Float(f.into())),
            ),
            "x_frac" => Some(
                self.x_frac
                    .map_or(IntrospectValue::Null, |f| IntrospectValue::Float(f.into())),
            ),
            "y_frac" => Some(
                self.y_frac
                    .map_or(IntrospectValue::Null, |f| IntrospectValue::Float(f.into())),
            ),
            "log" => Some(IntrospectValue::Text(
                self.reports
                    .iter()
                    .map(ButtonReport::label)
                    .collect::<Vec<_>>()
                    .join(";"),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Every field is a read-only projection of the input log.
            "report_count" | "last" | "last_button" | "last_edge" | "last_mods" | "last_x"
            | "last_y" | "x_frac" | "y_frac" | "log" => Err(InterveneError::ReadOnly),
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
        sink.raw_pointer_button(RawPointerButton {
            button,
            edge,
            modifiers,
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
            Some(IntrospectValue::Text(
                "left:down:;left:up:;middle:down:;middle:up:;right:down:;right:up:".to_owned()
            )),
            "all three buttons on both edges, each identified"
        );
        assert_eq!(sink.query("report_count"), Some(IntrospectValue::Int(6)));
    }

    #[test]
    fn the_press_edge_carries_modifiers() {
        // Gap B: the legacy PointerDown send wire dropped press modifiers; the
        // raw stream carries them on the DOWN edge.
        let mut sink = RawPointerSink::new();
        edge(&mut sink, PointerButton::Right, PointerEdge::Down, shift());
        assert_eq!(
            sink.query("last"),
            Some(IntrospectValue::Text("right:down:s".to_owned())),
            "the right PRESS carries the Shift modifier"
        );
        assert_eq!(
            sink.query("last_mods"),
            Some(IntrospectValue::Text("s".to_owned()))
        );
    }

    #[test]
    fn the_position_is_stamped_onto_each_report() {
        let mut sink = RawPointerSink::new();
        sink.pointer_move(0.25, 0.75);
        edge(
            &mut sink,
            PointerButton::Left,
            PointerEdge::Down,
            Modifiers::empty(),
        );
        assert_eq!(
            sink.query("last_x"),
            Some(IntrospectValue::Float(0.25_f32.into())),
            "the report stamps the live hover x"
        );
        assert_eq!(
            sink.query("last_y"),
            Some(IntrospectValue::Float(0.75_f32.into()))
        );
    }

    #[test]
    fn leaving_the_pane_clears_the_live_position_but_not_the_log() {
        let mut sink = RawPointerSink::new();
        sink.pointer_move(0.5, 0.5);
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
            Some(IntrospectValue::Null),
            "the live position clears on leave"
        );
        assert_eq!(
            sink.query("report_count"),
            Some(IntrospectValue::Int(1)),
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
        assert_eq!(sink.query("report_count"), Some(IntrospectValue::Int(1)));
        sink.invoke("clear", IntrospectValue::Null)
            .expect("clear is infallible");
        assert_eq!(sink.query("report_count"), Some(IntrospectValue::Int(0)));
        assert_eq!(sink.query("last"), Some(IntrospectValue::Null));
    }

    #[test]
    fn every_field_is_read_only() {
        let mut sink = RawPointerSink::new();
        for path in [
            "report_count",
            "last",
            "last_button",
            "last_mods",
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
        sink.pointer_move(0.3, 0.4);
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
