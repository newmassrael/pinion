//! `hello-scene-scale` — R1538 §5.16 §5.27: **the large-scene end-to-end
//! harness** for the pro-tool-performance axis.
//!
//! Every performance number this project holds is a component measured in
//! isolation: a shape-cache hit rate, a fragment-cache hit rate, an encode
//! span, a GPU span. None of them answers the axis's own question — *does this
//! framework hold 60fps with a large scene?* — and the obvious way to answer
//! it, timing a big binding and asserting a threshold, cannot be a CI guard:
//! a wall-clock assertion collides with the zero-flake policy, because the
//! number it reads belongs to the host.
//!
//! ## What is actually being claimed
//!
//! "60fps with large scenes" is not a claim about one machine's clock. It is a
//! **complexity** claim: per-frame work is bounded by what is *visible*, not
//! by how big the model is. That claim is machine-independent, and R1538 made
//! it readable — `scene/frame_timings` now carries the frame's node census
//! (`last.scene_nodes` / `last.layout_nodes` / `last.encode_nodes`). Those are
//! counts, so a guard over them is deterministic.
//!
//! This binding is what the guard drives. Its dataset size is settable at
//! runtime across four orders of magnitude, so one process can be asked the
//! question at both ends of the ladder — same window, same fonts, same caches,
//! nothing varying but `rows`.
//!
//! ## Why it also renders the WRONG thing on purpose
//!
//! A scale guard that can only ever measure the passing case cannot fail, and
//! a gate that cannot fail is worse than no gate (R1527). So the same binding
//! carries an **eager** arm — the same rows, built one scene node each, with
//! no windowing at all. It is a deliberate negative control:
//!
//! | arm | `rows` 100 → 1,000 | what the census does |
//! |---|---|---|
//! | `virtual` (default) | 1,000 → 1,000,000 | `scene_nodes` **flat** |
//! | `eager` | 100 → 1,000 (capped) | `scene_nodes` **×10** |
//!
//! The eager arm is capped at [`MAX_EAGER_ROWS`] because it means it: a
//! million eagerly-built rows is not a slow frame, it is a frame that does not
//! arrive. The cap is enforced by rejecting the write, not by silently
//! clamping — a guard that reads a clamped value learns nothing.
//!
//! ## The AI-first witness (§2 #2, §2 #7)
//!
//! Drive it entirely over RPC, no pixels:
//!
//! ```text
//! ui/invoke  {path: "/scale/external", method: "intervene",
//!             args: {path: "rows", value: 1000000}}
//! scene/frame_timings                      -> last.scene_nodes unchanged
//! ui/invoke  {... "eager" = true}          -> rejected while rows > cap
//! ```
//!
//! See `tools/demos/r1538_scene_scale.py`.
//!
//! ## a11y
//!
//! WAI-ARIA virtualized `list`: `aria-setsize = rows` on the container, one
//! `listitem` per row in the *visible window*, plus the two header buttons.
//! **Both arms report the same shape** — the eager arm's defect is that it
//! BUILDS every row, not that it announces them, and what an AT should hear is
//! the same either way. That is a stated scope, not an oversight: this round
//! is about the paint census.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y, windowed_list_nodes};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::virtual_list::view_virtual_list;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSceneScaleRenderer, HelloSceneScaleRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 520;
/// Shared [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key.
const THEME_TAG: &str = "app";
/// Primary [`SceneScaleExternal`] anchor, the paint root and the a11y `list`.
const LIST_TAG: &str = "scale";
/// Composite-tag region (R51.42): clicking it steps [`LADDER`].
const GROW_REGION: &str = "grow";
/// Composite-tag region: clicking it flips virtual ↔ eager.
const MODE_REGION: &str = "mode";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "scale_scroll";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "scale_scrollbar";

/// The dataset sizes the header button steps through, four orders of
/// magnitude apart. The guard reads the census at both ends: if the framework
/// windows correctly, the number does not move across the whole ladder.
const LADDER: [usize; 5] = [100, 1_000, 10_000, 100_000, 1_000_000];
/// The largest dataset the **eager** arm will accept.
///
/// Not a performance opinion — a bound on a thing that does not terminate
/// usefully. One scene node per row means a million rows is a million nodes
/// through view, layout, encode and the a11y walk, every frame. The cap is
/// enforced by *rejecting* the write, because a silently clamped value would
/// let a guard read `rows: 1000000` back from a binding holding a thousand.
const MAX_EAGER_ROWS: usize = 1_000;
/// Uniform per-row vertical slot in logical pixels; the windowing math is
/// exact integer division on it.
const ROW_PITCH: u32 = 28;
/// Extra rows built above + below the strict visible window so a fast flick
/// never exposes a blank gap.
const OVERSCAN: usize = 3;
/// Scroll viewport width (frames each row slot).
const VIEWPORT_W: u32 = 340;
/// Scroll viewport height — exactly 14 rows tall.
const VIEWPORT_H: u32 = 14 * ROW_PITCH;
const HEADER_FONT_PX: u32 = 13;
const ROW_FONT_PX: u32 = 13;

/// Widget state, projected out of the primary external by [`read_state`].
/// `Copy`, so the view stays a pure function of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScaleState {
    /// Dataset size — how many rows the model claims to have.
    rows: usize,
    /// `true` = build one scene node per row (the negative control);
    /// `false` = build only the visible window.
    eager: bool,
}

impl Default for ScaleState {
    fn default() -> Self {
        Self {
            rows: LADDER[1],
            eager: false,
        }
    }
}

/// Next rung of [`LADDER`] at or above `rows`, wrapping at the top. Rungs the
/// current arm cannot hold are skipped, so the button never proposes a size
/// the external is about to reject.
fn next_rung(rows: usize, eager: bool) -> usize {
    let ceiling = if eager { MAX_EAGER_ROWS } else { usize::MAX };
    let allowed: Vec<usize> = LADDER.into_iter().filter(|&n| n <= ceiling).collect();
    let at = allowed.iter().position(|&n| n == rows);
    match at {
        Some(i) => allowed[(i + 1) % allowed.len()],
        // Not on a rung (an `intervene` set an arbitrary size): start over.
        None => allowed[0],
    }
}

/// Synthetic row content. The seven-digit zero-pad keeps every row the same
/// width up to the top rung and makes the index unambiguous in a snapshot.
fn row_label(index: usize) -> String {
    const KINDS: [&str; 4] = ["mesh", "texture", "clip", "material"];
    format!("asset {index:07} \u{00B7} {}", KINDS[index % KINDS.len()])
}

/// One row: a zebra-striped strip carrying its index label, tagged
/// `scale#<i>` so the a11y `listitem` bounds resolve and a snapshot can name
/// exactly which indices the frame built.
fn build_row(index: usize, theme: &Theme) -> Scene {
    let fill = if index % 2 == 0 {
        theme.resolve(ColorRole::SurfaceContainerLow)
    } else {
        theme.resolve(ColorRole::SurfaceContainer)
    };
    let label = Scene::Text(TextNode::styled(
        row_label(index),
        Rect::default(),
        TextStyle::new()
            .with_size_px(ROW_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(format!("{LIST_TAG}#{index}"))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(VIEWPORT_W, ROW_PITCH))
                    .with_padding(Rect::new(10, 0, 10, 0)),
            ),
    )
}

/// A header strip that is also a click target, tagged `scale#<region>` so the
/// R51.42 composite-pointer arc routes it into the primary external.
fn header_button(region: &str, label: String, theme: &Theme) -> Scene {
    let text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(format!("{LIST_TAG}#{region}"))
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHigh),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(VIEWPORT_W / 2 - 4, 30)),
            ),
    )
}

/// view-fn (§6.3): pure sync `ScaleState -> Scene`.
///
/// The two arms differ in exactly one thing — whether the row builder runs for
/// the visible window or for the whole dataset — and the header above them is
/// identical, so a census difference between the arms is attributable to the
/// list and to nothing else.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "WidgetCore::view signature"
)]
fn view(state: ScaleState, _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();

    let list = if state.eager {
        // The negative control: one scene node per row, no windowing. The
        // scroll node still clips it, so what is on screen looks identical to
        // the virtual arm — which is the point. Only the census can tell them
        // apart, and telling them apart is the whole capability.
        let rows: Vec<Scene> = (0..state.rows).map(|i| build_row(i, &theme)).collect();
        Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
            Scene::Container(
                ContainerNode::new(rows)
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            ),
        ))
    } else {
        view_virtual_list(
            &scroll_state,
            Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
            state.rows,
            ROW_PITCH,
            OVERSCAN,
            |index| build_row(index, &theme),
        )
    };

    let scrollbar_style = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll_state,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    let controls = Scene::Container(
        ContainerNode::new(vec![
            header_button(GROW_REGION, format!("rows: {}", state.rows), &theme),
            header_button(
                MODE_REGION,
                if state.eager { "eager" } else { "virtual" }.to_string(),
                &theme,
            ),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::SpaceBetween)
                .with_size(Size::px(VIEWPORT_W, 30)),
        ),
    );

    let list_root = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_tag(LIST_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    Scene::Container(
        ContainerNode::new(vec![controls, list_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// Project the primary external's state out of the state scene.
fn read_state(scene: &Scene) -> ScaleState {
    let Some(intro) = scene
        .find_external_with_tag(LIST_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return ScaleState::default();
    };
    let rows = match intro.query("rows") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).unwrap_or(LADDER[1]),
        _ => LADDER[1],
    };
    let eager = matches!(intro.query("eager"), Some(IntrospectValue::Bool(true)));
    ScaleState { rows, eager }
}

// --- The scale external (primary) ------------------------------------------

/// Holds the dataset size and the arm. Everything this binding does is a
/// function of these two values, which is what keeps a census difference
/// attributable: nothing else about the frame changes when they do.
#[derive(Debug, Clone, Copy, Default)]
struct SceneScaleExternal {
    state: ScaleState,
}

impl SceneScaleExternal {
    fn new() -> Self {
        Self::default()
    }

    /// Apply a dataset size, refusing one the current arm cannot hold.
    ///
    /// Refusing rather than clamping is the load-bearing half: a guard that
    /// asked for a million eager rows and read back a thousand would have to
    /// infer the cap from the value, and a binding that clamps silently is
    /// indistinguishable from one that lied.
    fn set_rows(&mut self, rows: usize) -> Result<(), InterveneError> {
        if rows == 0 {
            return Err(InterveneError::OutOfRange);
        }
        if self.state.eager && rows > MAX_EAGER_ROWS {
            return Err(InterveneError::OutOfRange);
        }
        self.state.rows = rows;
        Ok(())
    }

    /// Switch arms. Entering the eager arm with a dataset it cannot hold is
    /// refused for [`Self::set_rows`]'s reason — the caller shrinks first, so
    /// the two facts are never changed by one write and a rejection always
    /// names one cause.
    fn set_eager(&mut self, eager: bool) -> Result<(), InterveneError> {
        if eager && self.state.rows > MAX_EAGER_ROWS {
            return Err(InterveneError::OutOfRange);
        }
        self.state.eager = eager;
        Ok(())
    }

    /// The R51.42 composite-pointer arc: `"<region>:<EventName>"`.
    fn handle_send(&mut self, payload: &str) {
        let Some((region, event)) = payload.split_once(':') else {
            return;
        };
        if event != "PointerUp" {
            return;
        }
        match region {
            GROW_REGION => self.state.rows = next_rung(self.state.rows, self.state.eager),
            MODE_REGION => {
                // Leaving `eager` always succeeds; entering it shrinks the
                // dataset to the cap first, because a click has no way to
                // report a refusal to the person who made it. The RPC path
                // refuses instead — a caller there CAN read the error.
                if self.state.eager {
                    self.state.eager = false;
                } else {
                    self.state.rows = self.state.rows.min(MAX_EAGER_ROWS);
                    self.state.eager = true;
                }
            }
            _ => {}
        }
    }
}

impl External for SceneScaleExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for SceneScaleExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // Dataset size. Writable — this is the axis the guard sweeps.
                    SchemaField::new("rows", "int"),
                    // Which arm builds the list. Writable.
                    SchemaField::new("eager", "bool"),
                    // The eager arm's ceiling, so a client reads the bound
                    // rather than discovering it by being refused.
                    SchemaField::new("max_eager_rows", "int"),
                    // The R51.42 composite pointer channel.
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "rows" => Some(IntrospectValue::Int(
                i64::try_from(self.state.rows).unwrap_or(i64::MAX),
            )),
            "eager" => Some(IntrospectValue::Bool(self.state.eager)),
            "max_eager_rows" => Some(IntrospectValue::Int(
                i64::try_from(MAX_EAGER_ROWS).unwrap_or(i64::MAX),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "rows" => match value {
                IntrospectValue::Int(i) => {
                    let rows = usize::try_from(i).map_err(|_| InterveneError::OutOfRange)?;
                    self.set_rows(rows)
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "eager" => match value {
                IntrospectValue::Bool(b) => self.set_eager(b),
                _ => Err(InterveneError::TypeMismatch),
            },
            "max_eager_rows" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    self.handle_send(payload);
                    Ok(IntrospectValue::Int(
                        i64::try_from(self.state.rows).unwrap_or(i64::MAX),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The binding -----------------------------------------------------------

struct SceneScaleView;

impl WidgetCore for SceneScaleView {
    type State = ScaleState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(SceneScaleExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![scrollbar_extra_external(
            use_scroll_state(SCROLL_KEY),
            SCROLLBAR_TAG,
        )]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    fn read_state(scene: &Scene) -> ScaleState {
        read_state(scene)
    }

    fn view(state: ScaleState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-scene-scale (R1538 §5.16 large-scene census)"
    }

    fn fmt_state_log(state: &ScaleState) -> String {
        format!(
            "rows={} arm={}",
            state.rows,
            if state.eager { "eager" } else { "virtual" }
        )
    }
}

impl WidgetA11y for SceneScaleView {
    fn access_node(state: &ScaleState, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let window = compute_visible_range(
            scroll_state.offset_y(),
            VIEWPORT_H,
            state.rows,
            ROW_PITCH,
            OVERSCAN,
        );
        let mut nodes = windowed_list_nodes(
            LIST_TAG,
            "Asset list",
            u32::try_from(state.rows).unwrap_or(u32::MAX),
            &window,
        );
        nodes.push(
            AccessNode::new(format!("{LIST_TAG}#{GROW_REGION}"), AriaRole::Button)
                .with_name(format!("Dataset size {}", state.rows)),
        );
        nodes.push(
            AccessNode::new(format!("{LIST_TAG}#{MODE_REGION}"), AriaRole::Button).with_name(
                if state.eager {
                    "Eager arm"
                } else {
                    "Virtual arm"
                },
            ),
        );
        nodes
    }
}

impl WidgetView for SceneScaleView {
    type Renderer = HelloSceneScaleRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<SceneScaleView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;

    fn render(state: ScaleState) -> Scene {
        Owner::new().run(|| view(state, &Frame::default()))
    }

    /// Count row containers `scale#<i>` present in a scene — an independent
    /// observation of what the view built, so the arm assertions below do not
    /// read a number the view handed them.
    fn row_nodes(scene: &Scene) -> usize {
        fn walk(scene: &Scene, out: &mut usize) {
            match scene {
                Scene::Container(c) => {
                    if c.tag
                        .as_deref()
                        .and_then(|t| t.strip_prefix(&format!("{LIST_TAG}#")))
                        .is_some_and(|rest| rest.parse::<usize>().is_ok())
                    {
                        *out += 1;
                    }
                    for ch in &c.children {
                        walk(ch, out);
                    }
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), out),
                _ => {}
            }
        }
        let mut n = 0;
        walk(scene, &mut n);
        n
    }

    #[test]
    fn r1538_the_virtual_arm_builds_the_same_rows_at_every_rung() {
        // The property the guard asserts over RPC, asserted here over the
        // view itself: four orders of magnitude of model, one window of rows.
        let counts: Vec<usize> = LADDER
            .into_iter()
            .map(|rows| row_nodes(&render(ScaleState { rows, eager: false })))
            .collect();
        assert!(
            counts.windows(2).all(|w| w[0] == w[1]),
            "virtual arm must be flat across the ladder, got {counts:?}",
        );
        assert!(counts[0] > 0, "and it must build SOME rows: {counts:?}");
    }

    #[test]
    fn r1538_the_eager_arm_builds_one_node_per_row() {
        // The negative control has to actually behave badly, or the guard's
        // discrimination is unverified: a test that only ever sees the good
        // arm cannot tell a working census from a constant.
        assert_eq!(
            row_nodes(&render(ScaleState {
                rows: 100,
                eager: true
            })),
            100,
        );
        assert_eq!(
            row_nodes(&render(ScaleState {
                rows: MAX_EAGER_ROWS,
                eager: true
            })),
            MAX_EAGER_ROWS,
        );
    }

    #[test]
    fn r1538_the_two_arms_diverge_only_in_how_many_rows_they_build() {
        // Same rung, same everything else. If the arms differed in the header
        // or the scrollbar too, a census difference would not be attributable
        // to the windowing.
        let virt = render(ScaleState {
            rows: MAX_EAGER_ROWS,
            eager: false,
        });
        let eager = render(ScaleState {
            rows: MAX_EAGER_ROWS,
            eager: true,
        });
        assert!(row_nodes(&virt) < row_nodes(&eager));
        for region in [GROW_REGION, MODE_REGION] {
            let tag = format!("{LIST_TAG}#{region}");
            assert!(
                find_tag(&virt, &tag) && find_tag(&eager, &tag),
                "{tag} must exist in both arms",
            );
        }
    }

    fn find_tag(scene: &Scene, want: &str) -> bool {
        match scene {
            Scene::Container(c) => {
                c.tag.as_deref() == Some(want) || c.children.iter().any(|ch| find_tag(ch, want))
            }
            Scene::Scroll(s) => find_tag(s.content.as_ref(), want),
            _ => false,
        }
    }

    #[test]
    fn r1538_the_eager_cap_refuses_rather_than_clamps() {
        // A clamped write is indistinguishable from a lie: the caller reads
        // back a value it did not ask for and has no way to know why.
        let mut ext = SceneScaleExternal::new();
        assert!(ext.set_eager(true).is_ok(), "1,000 rows is within the cap");
        assert_eq!(
            ext.intervene("rows", IntrospectValue::Int(1_000_000)),
            Err(InterveneError::OutOfRange),
        );
        assert_eq!(
            ext.query("rows"),
            Some(IntrospectValue::Int(
                i64::try_from(LADDER[1]).expect("rung fits i64")
            )),
            "and the refused write left the value alone",
        );

        // The mirror: a large dataset refuses to enter the eager arm.
        let mut ext = SceneScaleExternal::new();
        assert!(
            ext.intervene("rows", IntrospectValue::Int(1_000_000))
                .is_ok()
        );
        assert_eq!(
            ext.intervene("eager", IntrospectValue::Bool(true)),
            Err(InterveneError::OutOfRange),
        );
        assert_eq!(ext.query("eager"), Some(IntrospectValue::Bool(false)));
    }

    #[test]
    fn r1538_the_ladder_button_never_proposes_a_size_the_arm_refuses() {
        // The click path cannot report a refusal, so it must not produce one.
        for _ in 0..8 {
            let mut ext = SceneScaleExternal::new();
            ext.set_eager(true).expect("within cap");
            for _ in 0..8 {
                ext.handle_send(&format!("{GROW_REGION}:PointerUp"));
                assert!(
                    ext.state.rows <= MAX_EAGER_ROWS,
                    "eager ladder walked past its cap to {}",
                    ext.state.rows,
                );
            }
        }
        // And the virtual arm reaches the top rung.
        let mut ext = SceneScaleExternal::new();
        let mut seen = vec![ext.state.rows];
        for _ in 0..LADDER.len() {
            ext.handle_send(&format!("{GROW_REGION}:PointerUp"));
            seen.push(ext.state.rows);
        }
        assert!(
            seen.contains(&LADDER[LADDER.len() - 1]),
            "virtual ladder must reach the top rung, walked {seen:?}",
        );
    }

    #[test]
    fn r1538_state_round_trips_through_the_state_scene() {
        // `read_state` is the only path from the external to the view, so a
        // projection that dropped a field would make the whole harness read
        // its default forever.
        let mut ext = SceneScaleExternal::new();
        ext.intervene("rows", IntrospectValue::Int(100_000))
            .expect("virtual arm takes any size");
        assert_eq!(
            ext.query("rows"),
            Some(IntrospectValue::Int(100_000)),
            "the external holds what it was given",
        );
        assert_eq!(
            ext.query("max_eager_rows"),
            Some(IntrospectValue::Int(
                i64::try_from(MAX_EAGER_ROWS).expect("cap fits i64")
            )),
            "and publishes the bound rather than making a client discover it",
        );
    }
}
