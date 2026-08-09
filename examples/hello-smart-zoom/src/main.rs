//! R1435 §5.35 §5.15 — a native SMART-ZOOM gesture fits the block under the finger.
//!
//! A `PageView` [`External`] overrides [`External::smart_zoom_gesture`] — the toolkit native gesture event `SmartZoomNativeGesture` / macOS
//! `smartMagnifyWithEvent:` / winit `WindowEvent::DoubleTapGesture` peer — and toggles a three-block document page between
//! **fit-to-page** and **zoom-to-block**.
//!
//! This is the family's PHASE-LESS member, and the demo is built around what
//! that changes. The pinch / rotation / pan demos each accumulate a value across
//! a `Begin..End` arc and revert it on `Cancel`; there is no arc here — the
//! platform reports one completed toggle, so each gesture is one committed state
//! change with nothing to accumulate and nothing to discard. What the payload
//! lacks, the ANCHOR supplies: `y_rel` picks WHICH block fills the view, which
//! is the whole meaning of "smart" zoom (fit the object under the finger, not
//! merely scale the page).
//!
//! winit surfaces `WindowEvent::DoubleTapGesture` only on macOS / iOS, so the
//! `scene/smart_zoom_gesture` RPC is the AI-first driver (§2 #2): a headless
//! client zooms the page with no trackpad, and reads the state back through the
//! introspect surface (§2 #7) — the focused block's paint rect and the
//! `focused_block` field agree.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{Border, BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSmartZoomRenderer, HelloSmartZoomRendererError);

const WIN_W: u32 = 640;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";

/// The page's paint tag **and** the [`PageView`]'s registration tag — addressed
/// over RPC as `/external/<field>`. A transparent, pointer-opaque capture
/// surface over the page carries it, so a smart zoom anywhere over the page
/// resolves to the view (the router offers a native gesture to the widget under
/// the cursor).
const PAGE_TAG: &str = "page";

/// The human-readable report line at the window's foot.
const READOUT_TAG: &str = "zoom.readout";

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 13;

/// Window-absolute page region. The capture surface covers exactly this rect, so
/// the gesture's `y_rel` fraction `0.0..=1.0` maps to a block index.
const PAGE_RECT: Rect = Rect::new(20, 56, WIN_W - 40, WIN_H - 130);

/// The document's blocks — the objects a smart zoom can fit. Three is enough to
/// prove the anchor SELECTS one (a two-block page could not distinguish
/// "picked the block under the finger" from "picked the nearest edge").
const BLOCK_COUNT: usize = 3;

/// Gap between blocks when the page is laid out fit-to-page (logical px).
const BLOCK_GAP: u32 = 12;

/// A block's paint tag: `zoom.block.<i>`. A snapshot reads these rects to
/// confirm the zoom, so the picture and the `focused_block` field agree.
fn block_tag(index: usize) -> &'static str {
    match index {
        0 => "zoom.block.0",
        1 => "zoom.block.1",
        _ => "zoom.block.2",
    }
}

/// The block rect for the fit-to-page layout: the three blocks stack down the
/// page with [`BLOCK_GAP`] between them.
fn fit_block_rect(index: usize) -> Rect {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "BLOCK_COUNT is 3 and the page is a few hundred px; the arithmetic cannot overflow u32"
    )]
    let count = BLOCK_COUNT as u32;
    let total_gap = BLOCK_GAP * (count - 1);
    let h = (PAGE_RECT.h - total_gap) / count;
    #[allow(clippy::cast_possible_truncation, reason = "index < BLOCK_COUNT = 3")]
    let i = index as u32;
    Rect::new(
        PAGE_RECT.x,
        PAGE_RECT.y + i * (h + BLOCK_GAP),
        PAGE_RECT.w,
        h,
    )
}

/// The block rect when THIS block is the zoom target: it fills the page. That is
/// the fit-to-object contract — a smart zoom does not scale the page around a
/// point, it brings one object up to the viewport.
fn zoomed_block_rect() -> Rect {
    PAGE_RECT
}

/// The lower `y_rel` bound of each block: block `i` owns `BLOCK_BOUNDS[i]..`.
/// Written as literals rather than derived by casting [`BLOCK_COUNT`] to a float
/// — the cast is a precision-loss lint, and for three blocks the derivation buys
/// nothing over the table it would produce.
const BLOCK_BOUNDS: [f32; BLOCK_COUNT] = [0.0, 1.0 / 3.0, 2.0 / 3.0];

/// Which block a gesture at `y_rel` selects. The anchor is the entire payload of
/// a smart zoom, and this function is where it earns the name: the fraction down
/// the page picks the object under the finger. Out-of-range anchors clamp to the
/// first / last block rather than selecting nothing.
fn block_at(y_rel: f32) -> usize {
    let fraction = y_rel.clamp(0.0, 1.0);
    BLOCK_BOUNDS
        .iter()
        .rposition(|&lower| fraction >= lower)
        .unwrap_or(0)
}

// --- The paint-facing state snapshot ---------------------------------------

/// The read-only projection the binding paints, read back off the [`PageView`]'s
/// introspect surface — the SAME fields the AI-first `scene/query` client sees.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct ViewState {
    /// The block filling the page, or `None` when the page is fit-to-page.
    focused: Option<usize>,
    /// The number of smart-zoom gestures received (monotone until `reset`).
    events: u64,
    /// The anchor fraction of the last gesture (`None` before the first).
    anchor: Option<(f32, f32)>,
}

/// The idle prompt / live report line — the SSOT both the status text and the
/// a11y value read.
fn readout_text(state: &ViewState) -> String {
    let anchor = match state.anchor {
        Some((x, y)) => format!(" @ ({x:.2}, {y:.2})"),
        None => String::new(),
    };
    match state.focused {
        None if state.events == 0 => {
            "smart-zoom (two-finger double tap) to fit the block under the cursor".to_owned()
        }
        None => format!("fit to page — {} gesture(s){anchor}", state.events),
        Some(i) => format!("zoomed to block {i} — {} gesture(s){anchor}", state.events),
    }
}

// --- The page view (primary External) ---------------------------------------

/// The zoom authority. Overrides [`External::smart_zoom_gesture`] and toggles
/// between fit-to-page and zoom-to-block.
#[derive(Debug, Default)]
struct PageView {
    /// The block filling the page, or `None` for fit-to-page.
    focused: Option<usize>,
    /// The number of gestures received (monotone until `reset`).
    events: u64,
    /// The anchor fraction of the last gesture (`None` before the first).
    anchor: Option<(f32, f32)>,
    /// The modifiers held on the last gesture.
    modifiers: Modifiers,
}

impl PageView {
    fn new() -> Self {
        Self::default()
    }

    /// The toggle a smart zoom performs, stated once so the hook and the tests
    /// read the same rule:
    ///
    /// * fit-to-page → zoom to the block under the anchor;
    /// * zoomed to THAT block → back to fit-to-page (the tap-again restore);
    /// * zoomed to a DIFFERENT block → re-target, staying zoomed.
    ///
    /// The third case is why the anchor cannot be dropped once zoomed: a
    /// smart zoom is "fit what is under my finger", not a binary in/out.
    fn toggle_at(&mut self, y_rel: f32) {
        let target = block_at(y_rel);
        self.focused = match self.focused {
            Some(current) if current == target => None,
            _ => Some(target),
        };
    }
}

impl External for PageView {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// R1435 — the seam under test. One completed toggle per call: no phase to
    /// bracket, no delta to accumulate, and nothing a cancel could discard.
    /// Always consumes — the page owns the gesture.
    fn smart_zoom_gesture(&mut self, x_rel: f32, y_rel: f32, modifiers: Modifiers) -> bool {
        self.events += 1;
        self.anchor = Some((x_rel.clamp(0.0, 1.0), y_rel.clamp(0.0, 1.0)));
        self.modifiers = modifiers;
        self.toggle_at(y_rel);
        true
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for PageView {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The block filling the page, or Null when fit-to-page.
                    SchemaField::new("focused_block", "int"),
                    // Whether the page is zoomed at all (the Null-free read).
                    SchemaField::new("zoomed", "bool"),
                    // The smart-zoom gesture count (monotone until `reset`).
                    SchemaField::new("events", "int"),
                    // The anchor fraction of the last gesture (Null before the first).
                    SchemaField::new("anchor_x", "float"),
                    SchemaField::new("anchor_y", "float"),
                    // The modifiers held at the last gesture, as a wire token.
                    SchemaField::new("last_mods", "string"),
                    // Restore fit-to-page — the AI-first peer of a zoom reset.
                    SchemaField::new("reset", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "focused_block" => Some(self.focused.map_or(IntrospectValue::Null, |i| {
                IntrospectValue::Int(i64::try_from(i).unwrap_or(i64::MAX))
            })),
            "zoomed" => Some(IntrospectValue::Bool(self.focused.is_some())),
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
            "focused_block" | "zoomed" | "events" | "anchor_x" | "anchor_y" | "last_mods" => {
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
            // Restore fit-to-page (the AI-first peer of a zoom reset).
            "reset" => {
                *self = PageView::new();
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The view ---------------------------------------------------------------

fn read_page(scene: &Scene) -> ViewState {
    let Some(intro) = scene
        .find_external_with_tag(PAGE_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return ViewState::default();
    };
    let focused = match intro.query("focused_block") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    };
    let events = match intro.query("events") {
        Some(IntrospectValue::Int(n)) => u64::try_from(n).unwrap_or(0),
        _ => 0,
    };
    let anchor = match (query_frac(intro, "anchor_x"), query_frac(intro, "anchor_y")) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    ViewState {
        focused,
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

/// view-fn (§6.3): pure sync mapping of the page digest to a scene.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the WidgetCore::view trait hands the frame by reference; the signature mirrors it"
)]
fn view(state: ViewState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);
    let page_fill = theme.resolve(ColorRole::SurfaceContainerLow);
    let outline = theme.resolve(ColorRole::Outline);
    let accent = theme.resolve(ColorRole::Accent);

    let title = Scene::Text(
        TextNode::styled(
            "Smart-zoom to fit the block under the cursor — tap again to restore",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(20, 18)),
    );

    // The page body, behind the blocks and the capture surface.
    let page_body = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(page_fill).with_border(Border::new(outline, 1)),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(PAGE_RECT.x, PAGE_RECT.y)
                .with_size(Size::px(PAGE_RECT.w, PAGE_RECT.h)),
        ),
    );

    // The document blocks. Zoomed, ONLY the focused block is painted and it
    // fills the page — a snapshot therefore reads both halves of the state: the
    // focused block's rect grew, and the others are gone from the scene.
    let mut children = vec![page_body];
    for i in 0..BLOCK_COUNT {
        match state.focused {
            Some(f) if f == i => {
                children.push(block_scene(i, zoomed_block_rect(), accent, outline));
            }
            Some(_) => {}
            None => children.push(block_scene(i, fit_block_rect(i), page_fill, outline)),
        }
    }

    // Transparent, pointer-opaque capture surface over the page — the `page`
    // primary tag the External registers under. On top so a gesture anywhere
    // over the page resolves to it; transparent so the blocks show through.
    children.push(capture_surface(PAGE_TAG, PAGE_RECT, false));

    children.push(Scene::Text(
        TextNode::styled(
            readout_text(&state),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(on_surface_muted),
        )
        .with_tag(READOUT_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(20, WIN_H - 30)),
    ));
    children.push(title);

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// One document block at a window-absolute rect, tagged `zoom.block.<i>`.
fn block_scene(
    index: usize,
    rect: Rect,
    fill: pinion_core::style::Color,
    outline: pinion_core::style::Color,
) -> Scene {
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(fill).with_border(Border::new(outline, 2)),
        )
        .with_tag(block_tag(index))
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h)),
        ),
    )
}

// --- The binding ------------------------------------------------------------

struct SmartZoomView;

impl WidgetCore for SmartZoomView {
    type State = ViewState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(PageView::new())
    }

    fn tag() -> &'static str {
        PAGE_TAG
    }

    fn read_state(scene: &Scene) -> ViewState {
        read_page(scene)
    }

    fn view(state: ViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-smart-zoom (R1435 §5.35 native smart-zoom fit-to-block)"
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

impl WidgetA11y for SmartZoomView {
    fn access_node(state: &ViewState, _focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(PAGE_TAG, AriaRole::Group)
                .with_name("Document page")
                .with_value(AccessValue::Text(readout_text(state))),
        ]
    }
}

impl WidgetView for SmartZoomView {
    type Renderer = HelloSmartZoomRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<SmartZoomView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_anchor_selects_the_block() {
        // The fraction down the page picks the object under the finger — the
        // whole meaning of "smart". Edges clamp instead of running off the end.
        assert_eq!(block_at(0.0), 0);
        assert_eq!(block_at(0.2), 0);
        assert_eq!(block_at(0.5), 1);
        assert_eq!(block_at(0.9), 2);
        assert_eq!(block_at(1.0), 2, "the bottom edge is the last block");
        assert_eq!(block_at(1.5), 2, "an out-of-range anchor clamps");
        assert_eq!(block_at(-0.5), 0, "a negative anchor clamps");
    }

    #[test]
    fn a_gesture_zooms_and_a_second_restores() {
        let mut p = PageView::new();
        assert!(p.smart_zoom_gesture(0.5, 0.5, Modifiers::empty()));
        assert_eq!(p.focused, Some(1), "zoomed to the block under the anchor");
        // Tap again ON THE SAME block = restore fit-to-page.
        p.smart_zoom_gesture(0.5, 0.5, Modifiers::empty());
        assert_eq!(p.focused, None, "a second gesture restores fit-to-page");
        assert_eq!(p.events, 2, "each call is one completed toggle");
    }

    #[test]
    fn zoomed_to_another_block_retargets_instead_of_restoring() {
        // The case that separates a smart zoom from a binary in/out toggle:
        // while zoomed to block 0, a gesture over block 2 moves the zoom there
        // rather than zooming out.
        let mut p = PageView::new();
        p.smart_zoom_gesture(0.5, 0.1, Modifiers::empty());
        assert_eq!(p.focused, Some(0));
        p.smart_zoom_gesture(0.5, 0.9, Modifiers::empty());
        assert_eq!(p.focused, Some(2), "re-targets, stays zoomed");
        p.smart_zoom_gesture(0.5, 0.9, Modifiers::empty());
        assert_eq!(p.focused, None, "and the same block again restores");
    }

    #[test]
    fn anchor_and_modifiers_are_recorded() {
        let mut p = PageView::new();
        let shift = Modifiers {
            shift: true,
            ..Modifiers::empty()
        };
        p.smart_zoom_gesture(0.75, 0.25, shift);
        assert_eq!(p.anchor, Some((0.75, 0.25)));
        assert!(p.modifiers.shift_key(), "modifier bit recorded");
        assert!(matches!(
            p.query("zoomed"),
            Some(IntrospectValue::Bool(true))
        ));
        assert!(matches!(
            p.query("focused_block"),
            Some(IntrospectValue::Int(0))
        ));
    }

    #[test]
    fn reset_restores_fit_to_page() {
        let mut p = PageView::new();
        p.smart_zoom_gesture(0.5, 0.5, Modifiers::empty());
        assert!(p.focused.is_some());
        p.invoke("reset", IntrospectValue::Null).unwrap();
        assert_eq!(p.focused, None);
        assert_eq!(p.events, 0);
        assert!(matches!(
            p.query("focused_block"),
            Some(IntrospectValue::Null)
        ));
    }

    #[test]
    fn a_zoomed_block_fills_the_page_and_the_others_are_smaller() {
        // The paint side of the contract: fit-to-page blocks are a fraction of
        // the page, the zoomed one IS the page. A snapshot reads exactly this.
        let fit = fit_block_rect(1);
        let zoomed = zoomed_block_rect();
        assert!(
            fit.h < PAGE_RECT.h / 2,
            "a fit block is a fraction of the page"
        );
        assert_eq!(zoomed.h, PAGE_RECT.h, "the zoomed block fills the page");
        assert_eq!(zoomed.y, PAGE_RECT.y);
        // The fit layout stacks downward without overlapping.
        assert!(fit_block_rect(0).y < fit_block_rect(1).y);
        assert!(fit_block_rect(0).y + fit_block_rect(0).h <= fit_block_rect(1).y);
        assert!(fit_block_rect(2).y + fit_block_rect(2).h <= PAGE_RECT.y + PAGE_RECT.h);
    }
}
