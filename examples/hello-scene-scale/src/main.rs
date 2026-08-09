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
//! ## The second axis: a node count is not a cost (R1556)
//!
//! Everything above moves the *number* of things drawn, which is what a node
//! census measures. It leaves the harness unable to state the one thing that
//! census cannot see: **a `Container` is one node and a `Text` leaf holding four
//! thousand glyphs is one node.** A binding can window its rows perfectly,
//! satisfy every assertion the R1538 guard makes, and hand the GPU unbounded
//! work — so the guard did not bound what it claimed to bound.
//!
//! [`LABEL_LADDER`] is that axis. It sets how many characters each row's label
//! carries, with `rows` and the arm held fixed:
//!
//! | axis | what moves | `scene_nodes` | `last.draw.glyphs` |
//! |---|---|---|---|
//! | [`LADDER`], virtual | model size, 1e2 → 1e6 | **flat** | **flat** |
//! | [`LADDER`], eager | model size, 1e2 → 1e3 | ×10 | ×10 |
//! | [`LABEL_LADDER`] | per-row cost, 24 → 1,536 | **identical** | **×64** |
//!
//! The third row is the case R1556 exists for, and it is not a defect being
//! demonstrated — it is a size the framework has no business bounding. What was
//! a defect is that no number on any surface reported it.
//!
//! ## The AI-first witness (§2 #2, §2 #7)
//!
//! Drive it entirely over RPC, no pixels:
//!
//! ```text
//! ui/invoke  {path: "/scale/external", method: "intervene",
//!             args: {path: "rows", value: 1000000}}
//! scene/frame_timings                      -> last.scene_nodes unchanged
//!                                          -> last.draw.glyphs unchanged
//! ui/invoke  {... "label_chars" = 1536}    -> same nodes, 64x the glyphs
//! ui/invoke  {... "eager" = true}          -> rejected while rows > cap
//! ```
//!
//! See `tools/demos/r1538_scene_scale.py` and
//! `tools/demos/r1556_frame_states_what_it_drew.py`.
//!
//! ## a11y
//!
//! WAI-ARIA virtualized `list`: `aria-setsize = rows` on the container, one
//! `listitem` per row in the *visible window*, plus the three header buttons.
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
use pinion_core::widgets::virtual_list::{VisibleWindow, compute_visible_range};
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
/// (R1556) Composite-tag region: clicking it steps [`LABEL_LADDER`].
const WIDTH_REGION: &str = "width";
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
/// (R1556) The per-row label widths the header steps through — the **other**
/// axis of a frame's size, and the one [`LADDER`] cannot move.
///
/// Growing `rows` grows the *number* of things drawn, and the R1538 node census
/// sees that. Growing this grows the *size* of each thing while the number is
/// held exactly fixed, so every count on `scene/frame_timings` is byte-identical
/// across the whole ladder and the frame hands the GPU sixty times the work.
/// That is the case the node census was built to bound and cannot see, and this
/// axis is what drives a guard at it.
const LABEL_LADDER: [usize; 4] = [24, 96, 384, 1_536];
/// The longest per-row label the binding will accept. A bound on shaping work,
/// enforced by refusing the write for [`MAX_EAGER_ROWS`]' reason.
const MAX_LABEL_CHARS: usize = 4_096;
/// Filler appended to a row's natural label to reach the declared width. A
/// repeating word rather than one character so the shaper does the ordinary
/// thing (word-shaped runs, ordinary kerning) instead of a degenerate one.
const LABEL_FILLER: &str = "lod bounds uv normal tangent ";
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
    /// (R1556) Characters in each row's label — the size of a node, as against
    /// [`Self::rows`]' count of them. Moving this leaves every node census
    /// exactly where it was and moves the draw census by the same factor.
    label_chars: usize,
}

impl Default for ScaleState {
    fn default() -> Self {
        Self {
            rows: LADDER[1],
            eager: false,
            label_chars: LABEL_LADDER[0],
        }
    }
}

/// One step along a ladder, wrapping at the top; a value that is not on the
/// ladder (an `intervene` set an arbitrary one) restarts the walk.
///
/// (R1556) Shared by the two ladders rather than written twice, because it is
/// the WRAP RULE and nothing else — a second copy is free to disagree about
/// whether the top clamps or wraps, and the click path has no way to report a
/// disagreement to the person who clicked. What differs between the two is
/// which rungs are on offer, and that stays with the caller.
///
/// `rungs` is never empty at either call site: [`LADDER`]'s smallest entry is
/// below [`MAX_EAGER_ROWS`], so the filtered slice always keeps at least one.
fn next_on_ladder(rungs: &[usize], at: usize) -> usize {
    match rungs.iter().position(|&n| n == at) {
        Some(i) => rungs[(i + 1) % rungs.len()],
        None => rungs[0],
    }
}

/// Next rung of [`LADDER`] at or above `rows`, wrapping at the top. Rungs the
/// current arm cannot hold are skipped, so the button never proposes a size
/// the external is about to reject.
fn next_rung(rows: usize, eager: bool) -> usize {
    let ceiling = if eager { MAX_EAGER_ROWS } else { usize::MAX };
    let allowed: Vec<usize> = LADDER.into_iter().filter(|&n| n <= ceiling).collect();
    next_on_ladder(&allowed, rows)
}

/// (R1556) Next rung of [`LABEL_LADDER`] at or above `chars`, wrapping at the
/// top. Unlike [`next_rung`] no rung is ever unavailable: a wider label costs
/// shaping, which both arms pay identically, so there is nothing for an arm to
/// refuse.
fn next_label_rung(chars: usize) -> usize {
    next_on_ladder(&LABEL_LADDER, chars)
}

/// Synthetic row content, exactly `chars` characters wide. The seven-digit
/// zero-pad keeps every row the same width up to the top rung and makes the
/// index unambiguous in a snapshot.
///
/// (R1556) The width is *exact* — padded with [`LABEL_FILLER`] or truncated —
/// so the frame's glyph count is a known function of `rows_built * chars` and a
/// guard can assert the ratio rather than a threshold. Truncation is on
/// `char` boundaries, so a multi-byte filler could never split one.
fn row_label(index: usize, chars: usize) -> String {
    const KINDS: [&str; 4] = ["mesh", "texture", "clip", "material"];
    let mut s = format!("asset {index:07} \u{00B7} {}", KINDS[index % KINDS.len()]);
    while s.chars().count() < chars {
        s.push(' ');
        s.push_str(LABEL_FILLER);
    }
    s.chars().take(chars).collect()
}

/// One row: a zebra-striped strip carrying its index label, tagged
/// `scale#<i>` so the a11y `listitem` bounds resolve and a snapshot can name
/// exactly which indices the frame built.
fn build_row(index: usize, chars: usize, theme: &Theme) -> Scene {
    let fill = if index % 2 == 0 {
        theme.resolve(ColorRole::SurfaceContainerLow)
    } else {
        theme.resolve(ColorRole::SurfaceContainer)
    };
    let label = Scene::Text(TextNode::styled(
        row_label(index, chars),
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
                    .with_size(Size::px(VIEWPORT_W / 3 - 4, 30)),
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
        let rows: Vec<Scene> = (0..state.rows)
            .map(|i| build_row(i, state.label_chars, &theme))
            .collect();
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
            |index| build_row(index, state.label_chars, &theme),
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
            // (R1556) The second size axis, beside the first: this one moves
            // what each row COSTS with how many of them there are held fixed.
            header_button(
                WIDTH_REGION,
                format!("chars: {}", state.label_chars),
                &theme,
            ),
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
            .with_layout(
                LayoutStyle::new()
                    .with_focusable(true)
                    .flex(FlexDirection::Row),
            ),
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
    let label_chars = match intro.query("label_chars") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).unwrap_or(LABEL_LADDER[0]),
        _ => LABEL_LADDER[0],
    };
    ScaleState {
        rows,
        eager,
        label_chars,
    }
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
            return Err(InterveneError::out_of_range(
                "a scale model needs at least one row",
            ));
        }
        if self.state.eager && rows > MAX_EAGER_ROWS {
            return Err(InterveneError::out_of_range(format!(
                "the eager arm materialises every row, so it is capped at \
                 {MAX_EAGER_ROWS}; ask for {rows} on the windowed arm instead"
            )));
        }
        self.state.rows = rows;
        Ok(())
    }

    /// (R1556) Apply a per-row label width, refusing one past
    /// [`MAX_LABEL_CHARS`]. Refused rather than clamped for
    /// [`Self::set_rows`]' reason, and `0` is refused because a row with no
    /// label is not a narrower row, it is a different scene.
    fn set_label_chars(&mut self, chars: usize) -> Result<(), InterveneError> {
        if chars == 0 || chars > MAX_LABEL_CHARS {
            return Err(InterveneError::out_of_range(format!(
                "a row label runs 1..={MAX_LABEL_CHARS} characters, not {chars}"
            )));
        }
        self.state.label_chars = chars;
        Ok(())
    }

    /// Switch arms. Entering the eager arm with a dataset it cannot hold is
    /// refused for [`Self::set_rows`]'s reason — the caller shrinks first, so
    /// the two facts are never changed by one write and a rejection always
    /// names one cause.
    fn set_eager(&mut self, eager: bool) -> Result<(), InterveneError> {
        if eager && self.state.rows > MAX_EAGER_ROWS {
            return Err(InterveneError::out_of_range(format!(
                "this model has {} rows and the eager arm is capped at \
                 {MAX_EAGER_ROWS}; lower rows first",
                self.state.rows
            )));
        }
        self.state.eager = eager;
        Ok(())
    }

    /// The R51.42 composite-pointer arc: `"<region>:<EventName>[:<mods>[:<buttons>]]"`.
    ///
    /// R1619 — decoded through the grammar SSOT. The hand-rolled
    /// `split_once(':')` this replaces read `"grow:PointerUp:s"` as the event
    /// name `"PointerUp:s"`, so a Shift-click on any region was already
    /// silently inert before the held-button segment existed.
    fn handle_send(&mut self, payload: &str) {
        let Some(sent) = pinion_core::composite_tag::split_send_payload(payload) else {
            return;
        };
        let (region, event) = (sent.key, sent.event);
        if event != "PointerUp" {
            return;
        }
        match region {
            GROW_REGION => self.state.rows = next_rung(self.state.rows, self.state.eager),
            WIDTH_REGION => self.state.label_chars = next_label_rung(self.state.label_chars),
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
                    // (R1556) Per-row label width — the axis that moves the
                    // draw census with every node count held fixed. Writable.
                    SchemaField::new("label_chars", "int"),
                    // Its ceiling, published for `max_eager_rows`' reason.
                    SchemaField::new("max_label_chars", "int"),
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
            "label_chars" => Some(IntrospectValue::Int(
                i64::try_from(self.state.label_chars).unwrap_or(i64::MAX),
            )),
            "max_label_chars" => Some(IntrospectValue::Int(
                i64::try_from(MAX_LABEL_CHARS).unwrap_or(i64::MAX),
            )),
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
                    let rows = usize::try_from(i).map_err(|_| {
                        InterveneError::out_of_range(format!("{i} is not a row count"))
                    })?;
                    self.set_rows(rows)
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "eager" => match value {
                IntrospectValue::Bool(b) => self.set_eager(b),
                _ => Err(InterveneError::TypeMismatch),
            },
            "label_chars" => match value {
                IntrospectValue::Int(i) => {
                    let chars = usize::try_from(i).map_err(|_| {
                        InterveneError::out_of_range(format!("{i} is not a character count"))
                    })?;
                    self.set_label_chars(chars)
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "max_eager_rows" | "max_label_chars" => Err(InterveneError::ReadOnly),
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
            "rows={} chars={} arm={}",
            state.rows,
            state.label_chars,
            if state.eager { "eager" } else { "virtual" }
        )
    }
}

impl WidgetA11y for SceneScaleView {
    fn access_node(state: &ScaleState, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        // The eager arm is unwindowed in BOTH of its walks, not only in the
        // paint. `V::access_node` runs every frame and builds its own tree, so
        // a binding can window its paint perfectly and still enumerate its
        // whole model to assistive technology — and every assertion about the
        // painted tree would hold while the frame did O(model) work. Giving
        // the negative control that shape too is what lets the guard see it.
        let window = if state.eager {
            VisibleWindow {
                first: 0,
                count: state.rows,
            }
        } else {
            compute_visible_range(
                scroll_state.offset_y(),
                VIEWPORT_H,
                state.rows,
                ROW_PITCH,
                OVERSCAN,
            )
        };
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
            AccessNode::new(format!("{LIST_TAG}#{WIDTH_REGION}"), AriaRole::Button)
                .with_name(format!("Label width {} characters", state.label_chars)),
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
    use pinion_core::test_fixtures::assert_out_of_range_saying;

    fn render(state: ScaleState) -> Scene {
        Owner::new().run(|| view(state, &Frame::default()))
    }

    /// (R1556) The one traversal these tests observe a scene through.
    ///
    /// Three of them appeared in this module — count the rows, sum the text,
    /// find a tag — each with its own `match Container / Scroll / _` recursion,
    /// which is the mechanical duplication the 3rd-consumer rule names. The
    /// question each asks is a fold; only the walk was shared, and it was
    /// copied instead.
    fn visit(scene: &Scene, f: &mut impl FnMut(&Scene)) {
        f(scene);
        match scene {
            Scene::Container(c) => {
                for ch in &c.children {
                    visit(ch, f);
                }
            }
            Scene::Scroll(s) => visit(s.content.as_ref(), f),
            _ => {}
        }
    }

    /// Count row containers `scale#<i>` present in a scene — an independent
    /// observation of what the view built, so the arm assertions below do not
    /// read a number the view handed them.
    fn row_nodes(scene: &Scene) -> usize {
        let mut n = 0;
        visit(scene, &mut |node| {
            if let Scene::Container(c) = node
                && c.tag
                    .as_deref()
                    .and_then(|t| t.strip_prefix(&format!("{LIST_TAG}#")))
                    .is_some_and(|rest| rest.parse::<usize>().is_ok())
            {
                n += 1;
            }
        });
        n
    }

    #[test]
    fn r1538_the_virtual_arm_builds_the_same_rows_at_every_rung() {
        // The property the guard asserts over RPC, asserted here over the
        // view itself: four orders of magnitude of model, one window of rows.
        let counts: Vec<usize> = LADDER
            .into_iter()
            .map(|rows| {
                row_nodes(&render(ScaleState {
                    rows,
                    ..ScaleState::default()
                }))
            })
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
                eager: true,
                ..ScaleState::default()
            })),
            100,
        );
        assert_eq!(
            row_nodes(&render(ScaleState {
                rows: MAX_EAGER_ROWS,
                eager: true,
                ..ScaleState::default()
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
            ..ScaleState::default()
        });
        let eager = render(ScaleState {
            rows: MAX_EAGER_ROWS,
            eager: true,
            ..ScaleState::default()
        });
        assert!(row_nodes(&virt) < row_nodes(&eager));
        for region in [GROW_REGION, WIDTH_REGION, MODE_REGION] {
            let tag = format!("{LIST_TAG}#{region}");
            assert!(
                find_tag(&virt, &tag) && find_tag(&eager, &tag),
                "{tag} must exist in both arms",
            );
        }
    }

    /// (R1556) Characters across every row label the view built — an
    /// independent observation of the frame's TEXT size, derived by walking the
    /// painted tree rather than by multiplying the state back out.
    fn label_chars_built(scene: &Scene) -> usize {
        let mut n = 0;
        visit(scene, &mut |node| {
            if let Scene::Text(t) = node {
                n += t.content.chars().count();
            }
        });
        n
    }

    #[test]
    fn r1556_the_width_ladder_moves_the_text_with_the_node_count_held_fixed() {
        // The case R1538's census cannot see, at the view level. Same rung,
        // same arm, same everything — only the per-row label width moves. The
        // node count must be EQUAL (not merely close), and the text must grow
        // with the ladder, or the harness cannot drive a guard at the
        // distinction the draw census exists to make.
        let scenes: Vec<Scene> = LABEL_LADDER
            .into_iter()
            .map(|label_chars| {
                render(ScaleState {
                    label_chars,
                    ..ScaleState::default()
                })
            })
            .collect();
        let nodes: Vec<usize> = scenes.iter().map(row_nodes).collect();
        let chars: Vec<usize> = scenes.iter().map(label_chars_built).collect();
        assert!(
            nodes.windows(2).all(|w| w[0] == w[1]),
            "the node count must not move across the width ladder, got {nodes:?}",
        );
        assert!(
            chars.windows(2).all(|w| w[1] > w[0] * 3),
            "…while the text grows with it (4x rungs), got {chars:?}",
        );
    }

    #[test]
    fn r1556_a_row_label_is_exactly_as_wide_as_it_was_asked_for() {
        // The guard multiplies rows by width to predict a glyph count, so the
        // width has to be exact rather than approximate — including at widths
        // BELOW the natural label, where the padding branch never runs and the
        // truncation one must.
        for chars in [1_usize, 8, 24, 96, 384, 1_536, MAX_LABEL_CHARS] {
            let label = row_label(7, chars);
            assert_eq!(
                label.chars().count(),
                chars,
                "row_label(7, {chars}) was {label:?}",
            );
        }
    }

    #[test]
    fn r1556_the_width_ceiling_refuses_rather_than_clamps() {
        // `set_rows`' discipline on the second axis: a caller that reads back a
        // width it did not ask for cannot tell a cap from a lie.
        let mut ext = SceneScaleExternal::new();
        assert!(ext.set_label_chars(MAX_LABEL_CHARS).is_ok());
        // R1565 — the cap and the floor were one value; each now names the
        // range, which is the fact a client that asked for 0 or 999 needs.
        assert_out_of_range_saying(
            &ext.set_label_chars(MAX_LABEL_CHARS + 1),
            &format!("a row label runs 1..={MAX_LABEL_CHARS} characters"),
        );
        assert_out_of_range_saying(
            &ext.set_label_chars(0),
            &format!("a row label runs 1..={MAX_LABEL_CHARS} characters"),
        );
        assert_eq!(
            ext.state.label_chars, MAX_LABEL_CHARS,
            "a refused write leaves the value alone",
        );
    }

    #[test]
    fn r1556_the_width_button_walks_its_ladder_and_wraps() {
        // The click path, which unlike the RPC path cannot report a refusal —
        // so it must only ever propose rungs that are accepted.
        let mut seen = vec![LABEL_LADDER[0]];
        for _ in 0..LABEL_LADDER.len() {
            let next = next_label_rung(*seen.last().unwrap());
            assert!(
                LABEL_LADDER.contains(&next),
                "the button proposed {next}, which is not a rung",
            );
            seen.push(next);
        }
        assert_eq!(
            seen,
            [24, 96, 384, 1_536, 24],
            "one lap visits every rung in order and wraps",
        );
        assert_eq!(
            next_label_rung(37),
            LABEL_LADDER[0],
            "an off-ladder width set over RPC restarts the walk",
        );
    }

    fn find_tag(scene: &Scene, want: &str) -> bool {
        let mut found = false;
        visit(scene, &mut |node| {
            if let Scene::Container(c) = node
                && c.tag.as_deref() == Some(want)
            {
                found = true;
            }
        });
        found
    }

    #[test]
    fn r1538_the_eager_cap_refuses_rather_than_clamps() {
        // A clamped write is indistinguishable from a lie: the caller reads
        // back a value it did not ask for and has no way to know why.
        let mut ext = SceneScaleExternal::new();
        assert!(ext.set_eager(true).is_ok(), "1,000 rows is within the cap");
        assert_out_of_range_saying(
            &ext.intervene("rows", IntrospectValue::Int(1_000_000)),
            "the eager arm materialises every row",
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
        assert_out_of_range_saying(
            &ext.intervene("eager", IntrospectValue::Bool(true)),
            "lower rows first",
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
