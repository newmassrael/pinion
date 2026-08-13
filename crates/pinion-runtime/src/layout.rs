//! Runtime layout pass (§5.21 R23/R24): translates the §5.3
//! [`LayoutStyle`] sidecars on a [`Scene`] tree into a taffy compute
//! over a flex / block layout tree, then writes the resulting pixel
//! rects back into each node's `rect` field.
//!
//! `pinion-core` stays free of any taffy dependency per the §5.21
//! spec; the wrapper [`LayoutStyle`] enum set is what `pinion-core`
//! exports, and this module owns the translation into `taffy::Style`.
//!
//! R47.4 §5.36 — `Scene::Text` leaves carry a parley measure context
//! so taffy resolves their intrinsic width / height through the
//! [`pinion_text::LayoutCache`] passed into [`compute_layout`]. The
//! same cache is consumed by `paint_adapter::to_vello`'s Text arm on
//! the same frame, so each label shapes once and the result is reused
//! by both measure + paint passes.
//!
//! Single layout pass per frame; pure with respect to `(scene tree,
//! cache contents, viewport)` — same inputs produce identical rects.
//! The §6.3 view-fn purity invariant is preserved because nothing in
//! this module observes time or external state; the cache is content-
//! addressable (text + style + `max_width`), not time-keyed.
//!
//! R55.G.2 §5.45 — `Scene::Scroll` participates by getting its
//! content sub-tree laid out in a *separate* taffy pass: each Scroll
//! is treated as a layout leaf in the outer tree (its rect stays
//! app-set via `ScrollNode::viewport`), and the content beneath is
//! re-entered with [`compute_layout`] using `viewport.w` as the
//! cross-axis bound and `AvailableSpace::MaxContent` on the main
//! axis so flex children can overflow naturally instead of being
//! shrunk to fit the clip window. Content rects come out in
//! *content-local* coordinates (origin at the Scroll's content
//! origin, not absolute window space) — the hit-tester and paint
//! adapter translate via `Scroll.viewport.{x,y}` and `offset_{x,y}`
//! at read time, matching the §5.45 R55 substrate.

use pinion_core::Scene;
use pinion_core::scene::{
    BoxNode, ContainerNode, ExternalNode, ImageNode, PathNode, Rect, ScrollAxis, StyleRun, TextNode,
};
use pinion_core::style::{
    AlignItems, Display, FlexDirection, GridPlacement, GridTrack, JustifyContent, LayoutStyle,
    Overflow, SizeValue, TextStyle,
};
use taffy::geometry::{MinMax, Point as TaffyPoint};

use pinion_text::LayoutCache;
use std::collections::HashMap;
use taffy::prelude::{
    AvailableSpace, FromLength, FromPercent, LengthPercentage, LengthPercentageAuto,
    Line as TaffyLine, NodeId, Rect as TaffyRect, Size as TaffySize, TaffyGridLine, TaffyTree,
    auto, length, percent,
};
use taffy::style::{
    AlignItems as TaffyAlign, Dimension, Display as TaffyDisplay, FlexDirection as TaffyFlexDir,
    GridPlacement as TaffyGridPlacement, JustifyContent as TaffyJustify, MaxTrackSizingFunction,
    MinTrackSizingFunction, Overflow as TaffyOverflow, Position as TaffyPosition,
    Style as TaffyStyle, TrackSizingFunction,
};

/// R47.4 §5.36 — taffy `NodeContext` for leaves that need an intrinsic
/// measure callback. `Scene::Text` is the only consumer today; future
/// variants (image intrinsic / external opaque measure) extend this
/// enum without changing the closure shape.
pub enum NodeContext {
    /// `Scene::Text` leaf measure source — content + style + R713
    /// styled-run spans flow into `LayoutCache::layout_with_runs` to
    /// produce parley's intrinsic width / height. The styled runs
    /// participate because per-run font size / weight changes the
    /// shaped advances and line-break opportunities, so the measure
    /// pass must shape the same multi-style layout the paint pass will.
    /// The clones are necessary because the closure outlives the
    /// `&Scene` ref used during build.
    Text {
        content: String,
        style: TextStyle,
        runs: Vec<pinion_core::scene::StyleRun>,
        /// R1072 §5.37 — mirrors
        /// [`TextNode::caret_bearing`](pinion_core::scene::TextNode::caret_bearing)
        /// so the §5.37 measure override excludes editable text exactly as the
        /// paint arm does (shared eligibility SSOT).
        caret_bearing: bool,
    },
}

/// R1070 §5.37 — measure-override seam for the opt-in self-hosted text engine.
///
/// `layout` is feature-ungated, so it must not name the `vello`-gated
/// [`crate::text_engine::SelfHostedTextEngine`]; this trait is the decoupling
/// boundary — the same `enumerate ⊥ parse` layering R1067 drew between
/// `pinion-platform-fonts` (discovery) and `text_engine` (selection). The engine
/// implements it so a single-style `Scene::Text` leaf can be *sized* by the §5.37
/// engine, keeping the measured box self-consistent with the §5.37 paint arm
/// ([`crate::paint_adapter::to_vello_with_text_engine`]) — closing the R1068
/// paint-only gap where a string wider than the §5.37 advance overflowed the
/// parley measured box.
///
/// An absent override (`None` passed to [`compute_layout_with_text_measure`]) or a
/// `None` return is byte-identical to the pre-R1070 parley measure.
///
/// A named trait, not a bare `dyn Fn(..) -> Option<(f32, f32)>`: the §5.37 measure
/// surface is expected to grow (multi-line per-line metrics + baselines for
/// caller-side caret when styled-runs / multi-line route through §5.37 — R1072+),
/// so a documentable, ratified seam reads better than an anonymous closure and can
/// gain methods without becoming a second measure entry point. The single-method
/// `(f32, f32)` shape is the single-line interim; expect it to widen.
/// R1344 §5.36 §5.12 — a measured text box: its size plus the number of visual
/// lines it resolved into.
///
/// `line_count` is not decoration — it is the §5.12 `LayoutNode.line_count`
/// datum an AI client reads to verify text WITHOUT pixel inspection (§2 #7
/// scene-as-data), and only the measure impl knows it. The pre-R1344 seam
/// returned `(width, height)` and the caller hardcoded `1`, which was sound
/// while the only impl declined anything that would wrap (R1070's
/// `single_line_overflows`). The TUI's cell measure genuinely wraps, so a
/// hardcoded `1` became an active lie: a node 5 rows tall reporting "1 line",
/// with the two backends disagreeing on the exact datum §2 #7 exists to make
/// backend-neutral.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBox {
    /// Measured width in unrounded px.
    pub width: f32,
    /// Measured height in unrounded px.
    pub height: f32,
    /// Visual lines the content resolved into against `max_width` — the
    /// [`TextNode::line_count`](pinion_core::scene::TextNode::line_count)
    /// semantic (soft breaks induced by the width count; hard breaks count).
    pub line_count: u32,
    /// R1641 §5.21 — distance from the box's top edge down to the **first
    /// line's alphabetic baseline**, or `None` when this measure does not
    /// report one.
    ///
    /// [`AlignItems::Baseline`] is
    /// the consumer: a row aligning baselines needs, per item, the one number
    /// only the thing that shaped it knows. It joins `line_count` here for the
    /// reason `line_count` was added at R1344 — a fact the caller cannot
    /// re-derive from `(width, height)` and would otherwise have to guess.
    ///
    /// `Option` rather than a synthesized fallback: an impl that has no
    /// baseline says so, and its item then does not participate in the
    /// alignment. The alternative — reporting `height` and calling it a
    /// baseline — is indistinguishable at this type from a real one, so a
    /// measure that quietly stopped reporting would align every row slightly
    /// wrong instead of visibly not at all.
    pub baseline: Option<f32>,
    /// R1641.4 §5.36 §5.12 — the advance INCLUDING trailing whitespace, where
    /// [`Self::width`] excludes it.
    ///
    /// The two differ by exactly the trailing space a text engine declines to
    /// count toward a box, and until R1641.4 that difference was unreadable
    /// anywhere in the tree — a consumer met it as two labels rendering flush
    /// against each other and had to infer the cause from pixels. A measure
    /// that does not distinguish the two reports the same number twice, which
    /// is honest for a uniform cell grid and wrong for nothing.
    pub advance: f32,
}

impl TextBox {
    /// A single-line box — the shape an impl that never wraps always returns.
    ///
    /// Reports no baseline. An impl that knows one should build the struct and
    /// fill [`Self::baseline`]; this constructor stays the "size and nothing
    /// else" shortcut rather than inventing a number for it.
    #[must_use]
    pub const fn single_line(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            line_count: 1,
            baseline: None,
            advance: width,
        }
    }
}

pub trait TextMeasure {
    /// Measure a `Scene::Text` leaf, or return `None` to defer to the parley
    /// measure (every ineligible / declining case).
    ///
    /// `max_width` is taffy's resolved `Definite` available width (`None` for an
    /// unbounded min-/max-content probe). The returned `(width, height)` is the
    /// §5.37 line box in unrounded px; the caller applies the same integer ceil
    /// snapping it applies to the parley measure.
    ///
    /// R1072 §5.37 — `caret_bearing` is the leaf's
    /// [`TextNode::caret_bearing`](pinion_core::scene::TextNode::caret_bearing)
    /// marker: an editable [`TextField`] derives its caret / selection / hit-test
    /// geometry from a separate parley shaping, so a caret-bearing leaf must
    /// defer to parley for measure too (the eligibility SSOT folds it in, so
    /// measure and paint exclude editable text together).
    ///
    /// [`TextField`]: pinion_core::widgets::text_field
    fn measure_text(
        &self,
        content: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
        caret_bearing: bool,
    ) -> Option<TextBox>;
}

/// Compute the layout of `scene` against the given viewport extents.
///
/// `cache` is the application-owned [`LayoutCache`] (the same instance
/// the Vello paint adapter consults later in the frame). Shape work
/// done here populates the LRU so the subsequent
/// `paint_adapter::to_vello` call is a cache hit on every static
/// label. Mutates each node's `rect` field in place; nothing else is
/// touched. Safe to call every frame.
///
/// # Panics
///
/// Panics if taffy reports a tree-construction error; this can only
/// happen on internal logic bugs (passing invalid `NodeId`s, etc.),
/// not on any user-supplied scene shape.
#[allow(clippy::cast_precision_loss)]
pub fn compute_layout(
    scene: &mut Scene,
    cache: &mut LayoutCache,
    viewport_w: u32,
    viewport_h: u32,
) {
    let _ = compute_layout_with_scroll_dirty(scene, cache, viewport_w, viewport_h);
}

/// (R1458 §5.45 §5.27) How many view + layout passes one paint may spend
/// reaching the fixed point [`compute_layout_with_scroll_dirty`] reports,
/// before the caller gives up and carries the rest into the next frame.
///
/// Lives here rather than in either backend because both of them run the loop
/// (`pinion-shell`'s window paint and its RPC producer, `pinion-tui`'s paint —
/// §2 #6), and a budget that differed between them would mean a scene settled
/// in one backend and not the other.
///
/// Every known settling chain is short. The scroll bound and the pane rect each
/// converge in one re-pass (the value is layout-derived and then stable); a
/// windowed measured list needs one more, because the pass that pins the
/// viewport to a refined tail brings rows into the window that no earlier pass
/// had measured. `4` clears those with a pass to spare while still bounding a
/// binding whose view and layout disagree forever — that one is a bug in the
/// binding, and the honest response is to paint the best frame available and
/// keep the loop responsive, not to hang inside one paint.
///
/// # This number is a survey, and a proof is not available
///
/// R1460 — R1459 left "the budget is a survey of the chains that exist, not a
/// proof that none is longer" as an open debt. It is now closed as a decision,
/// because the proof cannot exist: a chain's length is a property of the
/// BINDING's view-to-layout feedback, which pinion does not author and cannot
/// bound. Any number here is a policy about when to stop, never a theorem.
///
/// What is achievable is that overrunning it is impossible to miss, and that
/// is what R1458 and R1459 built: the frame is bounded rather than hung, it
/// asks for the frame that continues (so the app stays responsive), it names
/// the binding in a log, and it publishes `settle_passes` + `settled` on
/// `scene/frame_timings` so the overrun is DATA. Evidence about this constant
/// therefore arrives as a number from a real binding — which is the only kind
/// of evidence that could justify changing it.
pub const SETTLE_PASS_BUDGET: u32 = 4;

/// What [`settle_to_fixed_point`] arrived at: the settled scene and the account
/// of getting there.
///
/// Named fields rather than a tuple: every caller forwards `passes` and
/// `settled` straight into a work counter alongside a shaper-miss count, and
/// positional results among same-shaped numbers are the transposition R1459
/// refused for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settled<S> {
    /// The scene the last executed pass laid out — never a freshly run view
    /// with no layout behind it, which would carry every rect at its default.
    pub scene: S,
    /// Passes spent, in `1..=`[`SETTLE_PASS_BUDGET`].
    pub passes: u32,
    /// Whether a pass found nothing left to change. `false` means the budget
    /// ran out with the chain still moving — read it WITH `passes`, since a
    /// chain that converges exactly ON the budget reports the same count.
    pub settled: bool,
}

/// (R1465 §5.45 §5.27 §2 #6) Run view → layout to a fixed point, bounded by
/// [`SETTLE_PASS_BUDGET`].
///
/// A layout pass writes state the view reads back — a scroll bound, a measured
/// row height, a pane's rect — so the scene it just laid out can already be out
/// of date. The loop that resolves that is four lines long and every one of
/// them is load-bearing: the last executed pass's scene is the one to keep, the
/// view is re-run only when another layout pass will follow it, the count is
/// the pass that ran and not the pass that was planned, and the whole thing is
/// bounded.
///
/// It lives here because getting it wrong is invisible. R1458 wrote it into the
/// three producers it found; R1460 discovered a fourth (the TUI's RPC producer)
/// running a single pass, and R1465 a fifth (the shell's stored paint-scene
/// mirror) — each one a copy that had been left behind, each found by a
/// consumer noticing a stale picture rather than by a test. Five copies of a
/// skeleton that must not drift is the case for having one.
///
/// The two closures are what legitimately differs: `run_view` produces a fresh
/// scene (which view fn, which window, whether a chrome inset applies), and
/// `layout` measures one into it and answers **whether this pass changed
/// anything the next view run would read**. Side effects that must happen on
/// every pass — the pane-viewport publish — belong inside `layout`, folded with
/// `|` so they run even when the layout itself came back clean.
///
/// # The disabled cascade rides here (R1554)
///
/// Every scene `run_view` produces goes through
/// [`resolve_disabled`](pinion_core::scene_disabled::resolve_disabled) before
/// it is laid out: the §5.39 inherited-disabled property is resolved, and a
/// region a container declared disabled has its ink faded.
///
/// It rides this loop for the reason the loop exists. The cascade must reach
/// every paint-scene producer in both backends — otherwise a window and a
/// terminal, or a live paint and its `scene/snapshot` mirror, would disagree
/// about which controls are inert — and this is the one place all five of them
/// already pass through. A producer cannot forget the cascade because there is
/// no longer anywhere to forget it: the return type is the settled scene, and
/// the only way to get one is from here.
///
/// The generic scene parameter became concrete [`Scene`] to do it. All five
/// call sites already produced one; the type variable was never carrying
/// anything.
pub fn settle_to_fixed_point(
    mut run_view: impl FnMut() -> Scene,
    mut layout: impl FnMut(&mut Scene) -> bool,
) -> Settled<Scene> {
    let mut scene = run_view();
    pinion_core::scene_disabled::resolve_disabled(&mut scene);
    let mut passes = 0_u32;
    for pass in 1..=SETTLE_PASS_BUDGET {
        passes = pass;
        if !layout(&mut scene) {
            return Settled {
                scene,
                passes,
                settled: true,
            };
        }
        if pass < SETTLE_PASS_BUDGET {
            scene = run_view();
            // Each pass lays out a FRESH view scene, so each needs its own
            // cascade. (The cascade is idempotent, so re-running it over an
            // already-resolved scene would be harmless — but a scene that
            // never had it run is not, and that is the case here.)
            pinion_core::scene_disabled::resolve_disabled(&mut scene);
        }
    }
    Settled {
        scene,
        passes,
        settled: false,
    }
}

/// (R1538 §5.16 §5.45) What one layout pass did: the dirty bit its caller
/// re-passes on, and **how many nodes it measured**.
///
/// # Why the node count rides the same value as the dirty bit
///
/// "60fps with large scenes" is not a wall-clock claim, it is a **complexity**
/// claim: per-frame work is bounded by what is *visible*, not by how big the
/// model is. `build_us` cannot state it — a machine-dependent duration says
/// nothing about how the work would grow — and a wall-clock assertion cannot
/// guard it without flaking. A node count can do both: it is deterministic, it
/// is the size of the thing the pass actually walked, and it is what stays flat
/// when a virtualized binding's row count grows by three orders of magnitude.
///
/// So it is returned rather than logged, and it is returned *here* because this
/// is the only value the pass already hands back. A separate accessor would let
/// a caller take the dirty bit and forget the count, which is how the count
/// would end up describing a different pass than the bit.
///
/// # It costs one integer read
///
/// A profiler that walks the tree to measure the tree is the cost it exists to
/// find. [`Self::nodes`] is `taffy`'s own `total_node_count()` plus each nested
/// scroll pass's — read once per pass, never walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutPass {
    /// Whether this pass mutated state the next `V::view` would read back — a
    /// scroll bound, a measured row height, a tail pin. `true` means the frame
    /// has **not** settled; see [`settle_to_fixed_point`].
    pub scroll_dirty: bool,
    /// How many layout nodes this pass measured: one per `Scene` node in the
    /// tree it was given, summed across the independent taffy pass every
    /// `Scene::Scroll`'s content gets.
    ///
    /// The census a scale claim is made of. A binding that windows its data
    /// reports the same number at a thousand rows and at a million; one that
    /// builds every row reports the model size, and that is the defect this
    /// exists to make visible before a user finds it.
    pub nodes: u32,
}

/// R57.X.scrollbar §5.45 — variant of [`compute_layout`] that returns
/// whether this pass *actually* mutated state the next `V::view` would read
/// back — a [`ScrollState::set_max`](pinion_core::widgets::scroll::ScrollState::set_max)
/// bound, a measured-row height, a tail pin's offset (post Signal
/// equality-skip). In one word: whether the frame has **not** settled.
///
/// Used by the shell's `compute_paint_scene`
/// substrate to detect the first-paint chicken-and-egg case where
/// `V::view` ran with the pre-layout `max = 0` snapshot and produced
/// a scrollbar widget rendering its track full. The shell re-runs
/// `V::view` + `compute_layout` while this returns `true` so the
/// scrollbar widget picks up the freshly-written max on the same
/// paint cycle — the user-visible "scrollbar fills the track on
/// startup" defect is what motivated the substrate exposure.
///
/// R1458 — *while*, not *once*: one re-pass is a fixed point only if no pass
/// can move the bound twice, and a windowed measured list moves it on the pass
/// after the one that scrolled new rows into view. A caller that stops after
/// one re-pass presents a half-settled scene and — having consumed the dirty
/// bit — leaves nothing to ask for the frame that would finish it. Callers run
/// to [`SETTLE_PASS_BUDGET`].
///
/// On steady-state paints the Signal equality-skip floors this at
/// `false`, so the loop short-circuits to a single pass — zero overhead on
/// every frame after the content size has settled.
///
/// R1538 — returns a [`LayoutPass`], whose `scroll_dirty` is the bit this
/// paragraph describes; `nodes` is the pass's node census.
///
/// # Panics
///
/// Same conditions as [`compute_layout`] (taffy internal logic
/// errors only, never on user-supplied scene shape).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn compute_layout_with_scroll_dirty(
    scene: &mut Scene,
    cache: &mut LayoutCache,
    viewport_w: u32,
    viewport_h: u32,
) -> LayoutPass {
    compute_layout_inner(scene, cache, viewport_w, viewport_h, None, None)
}

/// R1070 §5.37 — variant of [`compute_layout_with_scroll_dirty`] that accepts an
/// opt-in [`TextMeasure`] override.
///
/// When `text_measure` is `Some` and a `Scene::Text` leaf is eligible, the leaf
/// is sized by the §5.37 self-hosted engine so the measured box registers with
/// the §5.37 paint arm (real metric coherence: paint + measure self-consistent).
/// Every other leaf — and `text_measure == None` — measures through parley
/// exactly as [`compute_layout_with_scroll_dirty`], so passing `None` is
/// byte-identical to it. Returns the same scroll-dirty bit.
///
/// This is the seam the shell wires the engine through (R1071+); the parley path
/// stays the default everywhere else.
///
/// WIRE BOTH ARMS OR NEITHER: pair this with
/// [`to_vello_with_text_engine`](crate::paint_adapter::to_vello_with_text_engine)
/// using the SAME engine. Enabling the engine for measure but not paint (or
/// vice-versa) re-opens a coherence gap — a box sized by §5.37 filled by parley
/// glyphs, or a parley-sized box the §5.37 paint arm declines into. The shared
/// eligibility SSOT ([`crate::text_engine::self_hosted_text_eligible`]) guarantees
/// the two arms agree on WHICH leaves are eligible, not that both are enabled.
///
/// R1538 — returns the same [`LayoutPass`] as
/// [`compute_layout_with_scroll_dirty`].
///
/// # Panics
///
/// Same conditions as [`compute_layout`] (taffy internal logic errors only,
/// never on user-supplied scene shape).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn compute_layout_with_text_measure(
    scene: &mut Scene,
    cache: &mut LayoutCache,
    viewport_w: u32,
    viewport_h: u32,
    text_measure: Option<&dyn TextMeasure>,
) -> LayoutPass {
    compute_layout_inner(scene, cache, viewport_w, viewport_h, None, text_measure)
}

/// R55.G.2 §5.45 — extension point for laying out a `Scene::Scroll`
/// content sub-tree. `unbounded` selects which axis (if any) may
/// overflow the clip window: that axis's constraint is swapped from
/// `Definite(viewport extent)` to `MaxContent` so flex children grow
/// past the window (the scroll case) instead of being shrunk to fit;
/// the cross axis stays clamped to the viewport extent.
///
/// - `None` — the outer-window pass: both axes clamped to the
///   viewport (block defaults still fill).
/// - `Some(ScrollAxis::Vertical)` — height unbounded (the pre-R784
///   behaviour, the only scroll mode before horizontal landed).
/// - `Some(ScrollAxis::Horizontal)` — width unbounded (R784): the
///   content keeps its viewport height and may grow wider, so a
///   horizontally-scrolling container's content overflows sideways.
/// - `Some(ScrollAxis::Both)` — both axes unbounded (R877): a
///   pannable 2-D canvas declares its world extent and the measuring
///   pass must not clamp either dimension to the clip window.
///
/// # Panics
///
/// Panics if taffy reports a tree-construction error; this can only
/// happen on internal logic bugs (passing invalid `NodeId`s, etc.),
/// not on any user-supplied scene shape.
#[allow(clippy::cast_precision_loss)]
fn compute_layout_inner(
    scene: &mut Scene,
    cache: &mut LayoutCache,
    viewport_w: u32,
    viewport_h: u32,
    unbounded: Option<ScrollAxis>,
    text_measure: Option<&dyn TextMeasure>,
) -> LayoutPass {
    let width_unbounded = matches!(unbounded, Some(ScrollAxis::Horizontal | ScrollAxis::Both));
    let height_unbounded = matches!(unbounded, Some(ScrollAxis::Vertical | ScrollAxis::Both));
    let mut tree: TaffyTree<NodeContext> = TaffyTree::new();
    let mut baseline_rows: Vec<BaselineRow> = Vec::new();
    let layout_tree = build(scene, &mut tree, cache, text_measure, &mut baseline_rows);
    // R1641 — the offsets every baseline row can know WITHOUT a layout: its
    // text leaves'. Injected before the first pass so the common all-text row
    // is finished in one, and so the row's height is the engine's answer.
    for row in &baseline_rows {
        baseline_pass(row, &mut tree, false);
    }
    // Force the root to fill the viewport. The user's declared size
    // on the root is ignored at the top level; child sizing is the
    // user's domain. This mirrors how browsers treat `<html>`.
    let mut root_style = tree
        .style(layout_tree.node)
        .expect("root style query failed")
        .clone();
    // R55.G.2 §5.45 — scroll content lays out with auto height so the
    // flex Column total can overflow the clip window instead of
    // being clamped. The outer-window pass keeps the explicit
    // `length(viewport_h)` cap so block defaults still fill.
    //
    // R877 §5.45 — on an unbounded axis an *explicitly declared*
    // definite extent survives (the pannable canvas declares its world
    // size and positions children absolutely, so there is no intrinsic
    // child size for `auto` to measure — CSS likewise honours an
    // explicit size on an absolutely-positioning containing block).
    // Undeclared stays `auto` (the flex list/grid measuring path,
    // byte-identical to pre-R877).
    let declared = root_style.size;
    let keep_declared = |dim: Dimension| {
        if matches!(dim, Dimension::Length(_)) {
            dim
        } else {
            auto()
        }
    };
    root_style.size = TaffySize {
        width: if width_unbounded {
            keep_declared(declared.width)
        } else {
            length(viewport_w as f32)
        },
        height: if height_unbounded {
            keep_declared(declared.height)
        } else {
            length(viewport_h as f32)
        },
    };
    tree.set_style(layout_tree.node, root_style)
        .expect("set root style failed");
    let available = TaffySize {
        width: if width_unbounded {
            AvailableSpace::MaxContent
        } else {
            AvailableSpace::Definite(viewport_w as f32)
        },
        height: if height_unbounded {
            AvailableSpace::MaxContent
        } else {
            AvailableSpace::Definite(viewport_h as f32)
        },
    };
    // R51.1 §5.12 — side-channel `NodeId → line_count` table populated
    // by the measure callback, drained by `apply` into `TextNode.
    // line_count`. taffy's measure closure has no `&mut Scene` access
    // (Scene is borrowed read-only by `build`), and `NodeContext` is
    // owned by taffy without a mutable accessor on the path
    // `compute_layout_with_measure` returns, so a separate `HashMap`
    // bridges the measure pass to the apply pass. parley's `Layout::
    // lines().count()` is the shape backend agnostic source — the
    // `pinion_text::LayoutCache` swap to a self-hosted text engine
    // (§5.37.7 carry) keeps the same `.lines().count()` surface.
    let mut measured_text = MeasuredText::default();
    // R1641.4 §5.12 — the sibling side-channel, for the sibling datum: the
    // advance including trailing whitespace, which only the measure knows and
    // which `apply` drains into `TextNode::advance_px`.
    // R47.4 §5.36 — measure callback. Scene::Text leaves consult parley
    // (via `cache`) for intrinsic width / height; non-Text leaves
    // return `Size::ZERO`, matching the pre-R47.4 `compute_layout`
    // behaviour for variants without explicit `size` declarations.
    settle_layout(
        &mut tree,
        layout_tree.node,
        available,
        cache,
        text_measure,
        &baseline_rows,
        &mut measured_text,
    );
    apply(scene, &layout_tree, &tree, &measured_text, 0.0, 0.0);
    // R55.G.2 §5.45 — outer apply does not descend into `Scene::Scroll`
    // content (build also stops at Scroll), so any Scroll in the
    // tree now needs its content re-entered with its own taffy
    // pass. Content rects come out in scroll-local coordinates.
    // R1070 §5.37 — the same `text_measure` override flows into scrolled
    // content so a text leaf eligible for the §5.37 engine inside a Scroll is sized
    // by the engine too.
    // R1538 §5.16 — the pass's node census: taffy's own count for this tree,
    // plus every nested scroll pass's. Read from the tree, never walked — a
    // profiler that traverses the scene to size the scene is the cost it
    // exists to find.
    let nodes = u32::try_from(tree.total_node_count())
        .unwrap_or(u32::MAX)
        .saturating_add(lay_out_scroll_contents(scene, cache, text_measure));
    LayoutPass {
        scroll_dirty: post_layout_write_backs(scene),
        nodes,
    }
}

/// R1641.5 §5.12 — the per-node facts only the MEASURE knows, bridged from the
/// measure pass to the apply pass.
///
/// taffy's measure closure has no `&mut Scene` access and `NodeContext` has no
/// mutable accessor on the path `compute_layout_with_measure` returns, so these
/// travel beside the tree rather than on it. They are one struct because they
/// are always filled together and always drained together — carrying them as
/// separate parameters is what pushed two functions past the argument lint,
/// which was the lint noticing they are one thing.
#[derive(Default)]
struct MeasuredText {
    /// `NodeId -> TextNode::line_count` (R51.1 §5.12).
    lines: HashMap<NodeId, u32>,
    /// `NodeId -> TextNode::advance_px` (R1641.4 §5.12).
    advances: HashMap<NodeId, u32>,
}

/// R1641.5 §5.21 — lay the tree out, re-running while baseline offsets are
/// still moving.
///
/// Extracted from [`compute_layout_inner`] when the settle loop pushed that
/// function past the length lint. The lint was right: "measure and place every
/// node" and "keep placing until the baselines agree" are two jobs, and only
/// the second one has a termination argument to make.
fn settle_layout(
    tree: &mut TaffyTree<NodeContext>,
    root: NodeId,
    available: TaffySize<AvailableSpace>,
    cache: &mut LayoutCache,
    text_measure: Option<&dyn TextMeasure>,
    baseline_rows: &[BaselineRow],
    measured_text: &mut MeasuredText,
) {
    // R1641.5 §5.21 — the layout runs more than once ONLY when a baseline row
    // holds a child whose baseline has to be synthesized from its laid-out box
    // (CSS's bottom-margin-edge rule for an item that has no baseline of its
    // own). Everything else settles on the first pass and breaks out.
    //
    // Bounded rather than run-to-fixpoint: the offsets are top margins, and a
    // top margin does not change a cross-axis-Start item's own height, so the
    // second pass is exact and the loop normally exits after it. The bound
    // exists for the one shape that can still move — a percentage cross size,
    // which resolves against a container height the offsets DO change — where
    // stopping at a stated budget beats iterating on a tree the caller is
    // waiting for. The same budget the frame's settle loop uses.
    for pass in 0..SETTLE_PASS_BUDGET {
        tree.compute_layout_with_measure(
            root,
            available,
            |known_dimensions, available_space, node_id, node_context, _style| {
                if let TaffySize {
                    width: Some(width),
                    height: Some(height),
                } = known_dimensions
                {
                    return TaffySize { width, height };
                }
                match node_context {
                    Some(NodeContext::Text {
                        content,
                        style,
                        runs,
                        caret_bearing,
                    }) => {
                        // available_space.width.Definite → parley wrap point
                        // (multi-line); MinContent / MaxContent → no wrap
                        // (single line / unbounded), matching how taffy
                        // probes the leaf during flex resolution.
                        let max_width = match available_space.width {
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            AvailableSpace::Definite(w) if w.is_finite() && w >= 0.0 => {
                                Some(w as u32)
                            }
                            _ => None,
                        };
                        // R1070 §5.37 — opt-in self-hosted measure. A supplied engine
                        // override sizes an eligible single-style leaf by the §5.37
                        // metrics so the measured box registers with the §5.37 paint
                        // arm (closing the R1068 paint-only gap). `None` — no override,
                        // ineligible, or a single line that would soft-wrap — is the
                        // unchanged parley measure, so `text_measure == None` is
                        // byte-identical to the pre-R1070 path.
                        if let Some(measured) = text_measure.and_then(|tm| {
                            tm.measure_text(content, style, runs, max_width, *caret_bearing)
                        }) {
                            // R1344 §5.12 — the impl reports its own line count.
                            // Pre-R1344 this hardcoded `1` on the premise that "the
                            // §5.37 arm renders exactly one line by construction" —
                            // true of R1070's engine (it declines anything that would
                            // wrap), false of any measure that wraps, so the premise
                            // belongs to the impl, not to this call site.
                            measured_text.lines.insert(node_id, measured.line_count);
                            measured_text
                                .advances
                                .insert(node_id, ceil_px(measured.advance));
                            // R47.7.6 integer pixel snapping (see the parley branch).
                            TaffySize {
                                width: measured.width.ceil(),
                                height: measured.height.ceil(),
                            }
                        } else {
                            let layout = cache.layout_with_runs(content, style, runs, max_width);
                            // R51.1 §5.12 — capture line count on the last
                            // measure probe per node id; taffy may call this
                            // closure multiple times during flex resolution
                            // (MinContent / MaxContent / Definite). The final
                            // call uses the resolved Definite width, which is
                            // also what `apply` would re-measure against, so
                            // overwriting on every call is correct.
                            #[allow(clippy::cast_possible_truncation)]
                            let line_count = layout.lines().count() as u32;
                            measured_text.lines.insert(node_id, line_count);
                            // R1641.4 — `full_width()` is the shaper's own name for
                            // "including the trailing whitespace `width()` drops".
                            measured_text
                                .advances
                                .insert(node_id, ceil_px(layout.full_width()));
                            // R47.7.6 — integer pixel snapping. parley returns
                            // sub-pixel f32 widths; without `ceil` the value
                            // oscillates `77.0`/`77.8` between adjacent
                            // viewport widths, producing a visible 1-px text
                            // jitter on mouse-drag resize. `ceil` rounds toward
                            // "fits inside taffy's bound" so the result snaps
                            // monotonically and the cached `rect.w` stays stable
                            // across consecutive frames at the same content.
                            TaffySize {
                                width: layout.width().ceil(),
                                height: layout.height().ceil(),
                            }
                        }
                    }
                    None => TaffySize::ZERO,
                }
            },
        )
        .expect("taffy compute_layout failed");
        // Now every box has a size, so a row that was holding a
        // not-yet-synthesizable baseline can finish. `moved` is false on the
        // pass where nothing changed, which is what ends this for an all-text
        // tree after exactly one layout.
        let mut moved = false;
        for row in baseline_rows {
            moved |= baseline_pass(row, tree, true);
        }
        if !moved {
            break;
        }
        debug_assert!(
            pass + 1 < SETTLE_PASS_BUDGET,
            "baseline offsets did not settle within {SETTLE_PASS_BUDGET} passes",
        );
    }
}

/// R1538 §5.45 — the three post-layout write-backs, in the order their
/// contracts require, folded into the one dirty bit the settle loop re-passes
/// on.
///
/// Lifted out of [`compute_layout_inner`] because their **order** is the whole
/// content of this function and it was previously stated only by their
/// adjacency inside a hundred-line body. Named, the ordering constraint has
/// somewhere to live.
fn post_layout_write_backs(scene: &mut Scene) -> bool {
    // R55.G.5 §5.45 — automatic max-bound write. The content's
    // post-layout rect carries the true intrinsic size; pushing it
    // into the attached `ScrollState` here retires the pre-R55.G.5
    // chicken-and-egg workaround where every view fn had to
    // duplicate the row-count × row-height arithmetic and call
    // `set_max` manually before the layout pass had even run.
    //
    // R57.X.scrollbar §5.45 — bubble out whether any `set_max` write
    // actually mutated a bound (post Signal equality-skip). The
    // [`compute_layout_with_scroll_dirty`] caller (shell substrate)
    // uses this to detect the first-paint chicken-and-egg case and
    // schedule a same-frame re-pass so the scrollbar widget never
    // paints a stale full-track thumb to the user.
    let bounds_dirty = update_scroll_state_bounds(scene);
    // R1194 §5.27 — measured variable-height list feedback. Run *after*
    // the bounds pass so the harvest's grow-then-pin (which writes the
    // bound from the refined total) is the last word on `max`. Folds its
    // own dirty bit into the same first-paint / re-pass machinery: a frame
    // that changes a measured height re-runs the view against the refined
    // table (the measure→settle warmup).
    let harvest_dirty = harvest_measured_rows(scene);
    // R1445 §5.45 §5.27 — deferred tail pin. Runs *last* on purpose: both
    // writers above can move `max` this frame (the content-rect bound, then the
    // measured-row harvest's refinement), and the pin's contract is "the bound
    // this frame ended with", so it must not be able to observe an intermediate
    // one. Folds its own dirty bit into the same re-pass machinery — the pin
    // moves an offset the already-built scene was laid out against.
    let pin_dirty = apply_measured_tail_pins(scene);
    bounds_dirty || harvest_dirty || pin_dirty
}

/// R55.G.2 §5.45 — walks the scene and lays out each `Scene::Scroll`
/// content sub-tree as an independent taffy root sized by the
/// scroll's `viewport`. Recursion is handled by the inner
/// [`compute_layout_inner`] call's own tail invocation, so nested
/// Scrolls naturally cascade.
///
/// R1538 — returns how many layout nodes those inner passes measured, so the
/// caller's [`LayoutPass::nodes`] covers the whole tree and not just the part
/// taffy laid out in one go. Scrolled content is exactly where a large scene
/// lives, so a census that stopped at the clip window would report a flat
/// number for a binding that builds a million rows inside one.
fn lay_out_scroll_contents(
    scene: &mut Scene,
    cache: &mut LayoutCache,
    text_measure: Option<&dyn TextMeasure>,
) -> u32 {
    match scene {
        Scene::Container(c) => c.children.iter_mut().fold(0, |acc, child| {
            acc.saturating_add(lay_out_scroll_contents(child, cache, text_measure))
        }),
        Scene::Scroll(s) => {
            let vw = s.viewport.w;
            let vh = s.viewport.h;
            // R784 — unbound the scroll's own axis (vertical content
            // grows taller, horizontal content grows wider); the cross
            // axis stays clamped to the viewport.
            let axis = s.axis;
            // Discard the inner pass's scroll-dirty bit: the outer
            // pass's `update_scroll_state_bounds` walks the same
            // ScrollState (parent-Scroll) after this returns, so the
            // outer accumulator is the canonical source of truth. Its node
            // count is NOT discarded — that one has no outer equivalent,
            // because `build` stops at a Scroll.
            compute_layout_inner(s.content.as_mut(), cache, vw, vh, Some(axis), text_measure).nodes
        }
        _ => 0,
    }
}

/// R1445 §5.45 — visit every `Scene::Scroll` in the tree, OR-folding each
/// visit's dirty bit.
///
/// The traversal `update_scroll_state_bounds` / [`harvest_measured_rows`] /
/// [`apply_measured_tail_pins`] each carried a copy of (R1445 self-grep: the
/// tail pin was the third). Only the per-node work differed; the walk — fold
/// the children of a Container, visit a Scroll and then descend into its
/// content — was mechanical wiring repeated verbatim, so it lives here once.
///
/// Descends with `|`, not the `dirty || recurse(…)` the three copies each
/// ended on. That is a readability choice, **not** a bug fix: a nested scroll
/// is reached anyway, because [`lay_out_scroll_contents`] gives every scroll's
/// content its own [`compute_layout_inner`] pass, which runs these same three
/// walks over the sub-tree. No test in the suite distinguishes the two forms —
/// verified by restoring `||` and re-running. It is `|` here so the traversal
/// means what its name says for any future caller that is not already covered
/// by that inner pass; a short-circuit would become a live bug the moment one
/// exists, and would be silent.
fn fold_scrolls(
    scene: &Scene,
    visit: &mut impl FnMut(&pinion_core::scene::ScrollNode) -> bool,
) -> bool {
    match scene {
        Scene::Container(c) => {
            let mut dirty = false;
            for child in &c.children {
                // `|=`-fold so the walk continues after one descendant flips
                // the bit — every nested scroll gets its visit this pass
                // regardless of which one moved.
                dirty |= fold_scrolls(child, visit);
            }
            dirty
        }
        Scene::Scroll(s) => {
            let here = visit(s);
            // Descend into the content so nested scrolls are visited too;
            // `|` (not `||`) so `here` never suppresses the descent.
            here | fold_scrolls(s.content.as_ref(), visit)
        }
        _ => false,
    }
}

/// R55.G.5 §5.45 — walks the scene and, for every `Scene::Scroll`
/// with an attached `ScrollState`, writes the layout-derived max
/// bounds (`content_size - viewport_size`, clamped to 0). Called
/// after [`lay_out_scroll_contents`] so the content rect is
/// authoritative.
///
/// The `Signal::set` calls inside `ScrollState::set_max` are
/// equality-skipped (R51.149), so a steady-state frame with
/// unchanged content geometry does not schedule a paint.
#[allow(clippy::cast_possible_wrap)]
fn update_scroll_state_bounds(scene: &Scene) -> bool {
    fold_scrolls(scene, &mut |s| {
        let mut dirty = false;
        // R859 §5.45 — a linked-scroll *follower* shares its state
        // with a primary node that owns the bounds write; the
        // follower never publishes. Publishing from both would
        // flip-flop the shared `measured_*` (the frozen-column grid's
        // two vertical body scrolls share one vertical state but sit in
        // side-by-side columns of different cross-axis *widths*) and
        // spin a perpetual scroll-dirty re-pass. The follower still
        // lays out its content unbounded along its axis in
        // `lay_out_scroll_contents` (which is follower-agnostic), so
        // the overflow clip + offset slide still apply — only the
        // feedback is suppressed here.
        if let Some(state) = s.state.as_ref().filter(|_| !s.follower) {
            let content_rect = s.content.rect();
            // Content rect is scroll-local (origin at (0, 0)),
            // so `rect.w/h` already encode the intrinsic content
            // size — no `+ rect.x/y` accumulation needed.
            // R57.X.scrollbar §5.45 — `set_max` returns whether
            // either max bound actually mutated (post-Signal
            // equality-skip). On the very first paint of an
            // application's lifetime every ScrollState reads back
            // with `max == 0` so the first non-zero content-size
            // write flips this bit; the substrate uses the
            // accumulated bit to re-run `V::view` + this layout
            // pass on the same frame so the scrollbar widget
            // paints with the freshly-written max instead of a
            // full-track thumb.
            //
            // R996 §5.27 — each axis bound goes through the
            // `max_scroll_offset` SSOT (content extent − viewport,
            // clamped i32), shared with app-side pre-layout bound
            // writers (e.g. a streaming view that pins the viewport
            // to a freshly-appended tail in the same frame).
            dirty = state.set_max(
                pinion_core::widgets::scroll::max_scroll_offset(content_rect.w, s.viewport.w),
                pinion_core::widgets::scroll::max_scroll_offset(content_rect.h, s.viewport.h),
            );
            // R774 §5.27 — AutoSizer feedback. `apply` wrote the
            // flex-computed clip-window rect into `s.viewport`
            // above (R55.G.4 `assign_rect`), so this is the true
            // measured extent for a flex-sized scroll container.
            // Publishing it lets a flex-viewport virtualized list
            // (`view_flex_virtual_list`) window against the laid-
            // out height instead of a caller-supplied const. The
            // dirty bit folds into the same first-paint warmup as
            // `set_max`: the first paint windows an empty list
            // (height 0 → no rows), this write flips the bit, and
            // the shell re-runs `V::view` + layout on the same
            // frame with the true height. A fixed-size scroll
            // node's rect never moves, so this is a one-shot
            // no-op for non-flex consumers (Signal equality-skip).
            dirty |= state.set_measured_viewport(s.viewport.w, s.viewport.h);
        }
        dirty
    })
}

/// R1445 §5.45 §5.27 — walks the scene and, for every `Scene::Scroll` whose
/// [`ScrollState`](pinion_core::widgets::scroll::ScrollState) carries a standing
/// [`follow_measured_tail`](pinion_core::widgets::scroll::ScrollState::follow_measured_tail)
/// arming, pins the offset to the bound this frame published.
///
/// The layout-measured half of the grow-then-pin idiom
/// [`follow_tail`](pinion_core::widgets::virtual_list::follow_tail) serves
/// arithmetically: a consumer that appends content whose extent only taffy /
/// parley know arms the intent, and the bound it pins against is written above
/// — so the consumer never has to state (or fake) an extent it cannot compute.
///
/// A separate walk rather than a branch inside
/// [`update_scroll_state_bounds`] because the pin must observe the bound
/// **after** every writer of this frame, including
/// [`harvest_measured_rows`]'s refinement.
///
/// R859 followers are skipped, on the same gate the bounds pass uses: **the
/// pin rides the node that publishes the bound**. A follower shares its
/// primary's `ScrollState`, so a shared arming still fires exactly once — from
/// the primary, with the primary's declared axis. Letting a follower fire it
/// would make the result depend on scene order (whichever node the walk
/// reached first would decide which axes count as "the tail"), and would pin
/// against a bound the follower does not own. A state whose only node in this
/// scene *is* a follower keeps its arming standing — correctly: nobody
/// published its bound this frame either, so there is no measured tail to pin
/// to yet. That standing state is what `scene/scroll_state`'s
/// `following_measured_tail` reports.
///
/// Returns whether any pin moved an offset; a scroll with no arming is a pure
/// no-op, so this is opt-in.
fn apply_measured_tail_pins(scene: &Scene) -> bool {
    fold_scrolls(scene, &mut |s| {
        s.state
            .as_ref()
            .filter(|_| !s.follower)
            .is_some_and(|state| state.apply_measured_tail_pin(s.axis))
    })
}

/// R1194 §5.27 — walks the scene and, for every `Scene::Scroll` carrying a
/// [`MeasuredRowState`](pinion_core::widgets::measured_rows::MeasuredRowState),
/// harvests each windowed row's laid-out height into that state and keeps
/// the scroll anchored (the "layout-pass measurement round-trip" a measured
/// variable-height list needs).
///
/// Called after [`lay_out_scroll_contents`] so each row's `rect` is
/// authoritative. A scroll without `measured_rows` (every fixed-pitch or
/// caller-supplied-height list, i.e. the overwhelming majority) is a pure
/// no-op, so this is opt-in. Returns whether any measurement changed a
/// height — folded into the frame dirty bit so the view re-runs against the
/// refined table (the two-frame measure→settle warmup, identical to the
/// `update_scroll_state_bounds` first-paint re-pass).
fn harvest_measured_rows(scene: &Scene) -> bool {
    fold_scrolls(scene, &mut |s| {
        let mut dirty = false;
        // A measured list wires BOTH the measured-row state (harvest target)
        // and the scroll state (anchor + offset); `view_measured_list` always
        // does. Fail fast (debug) on a hand-built node that wired only
        // `measured_rows` — otherwise it would silently render on the
        // estimate forever (R1199 audit: fail-fast principle).
        if let Some(measured) = s.measured_rows.as_ref() {
            debug_assert!(
                s.state.is_some(),
                "a ScrollNode with measured_rows must also carry a ScrollState \
                     (the harvest needs it for the anchor + offset)",
            );
            if let Some(scroll) = s.state.as_ref() {
                let mut rows = Vec::new();
                collect_measured_rows(s.content.as_ref(), &mut rows);
                dirty = measured.harvest(scroll, s.viewport.h, rows);
            }
        }
        dirty
    })
}

/// R1194 §5.27 — collect `(row_index, laid_out_height)` for every node in a
/// measured list's content tagged `measured-row:<index>` (the tag
/// `view_measured_list` stamps on each windowed row slot). The slot is laid
/// out width-fixed / height-auto, so its `rect().h` is the row's natural
/// content height — exactly the value the measurement table wants. A
/// measured-row slot's own subtree is the row content (no nested measured
/// rows), so the walk does not descend into a matched slot.
fn collect_measured_rows(scene: &Scene, out: &mut Vec<(usize, u32)>) {
    if let Some(index) = scene
        .tag()
        .and_then(pinion_core::widgets::measured_rows::measured_row_index)
    {
        out.push((index, scene.rect().h));
        return;
    }
    match scene {
        Scene::Container(c) => {
            for child in &c.children {
                collect_measured_rows(child, out);
            }
        }
        Scene::Scroll(s) => collect_measured_rows(s.content.as_ref(), out),
        _ => {}
    }
}

/// Recursive shadow tree mirroring the Scene; each entry holds the
/// taffy `NodeId` and the children we registered for that node.
struct LayoutShadow {
    node: NodeId,
    children: Vec<LayoutShadow>,
}

/// R1641.4 — px, rounded up, clamped at zero.
///
/// The same `ceil` the measured box takes (R47.7.6): without it a sub-pixel
/// advance oscillates between adjacent integers across frames, and the whole
/// point of this number is that a client can compare it with `rect.w`, which
/// is integral. Rounding them differently would manufacture a trailing space
/// that is not there.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a text advance is a small non-negative px value after the clamp"
)]
fn ceil_px(v: f32) -> u32 {
    v.max(0.0).ceil() as u32
}

/// R1641 §5.21 §5.36 — where a child's first text baseline sits, measured from
/// the top of its own box, or `None` when it has none to report.
///
/// A [`Scene::Text`] leaf only. A container's baseline would be its first text
/// descendant's, offset by wherever layout put that descendant — a number that
/// does not exist yet at the point this is asked, which is before the layout
/// pass. See [`AlignItems::Baseline`] on why the shift is computed here rather
/// than as a correction afterwards.
///
/// Measured at unbounded width because a first line's baseline is a function of
/// the font metrics and the line-height policy, not of where the text wraps.
fn text_baseline_of(
    scene: &Scene,
    cache: &mut LayoutCache,
    text_measure: Option<&dyn TextMeasure>,
) -> Option<f32> {
    let Scene::Text(t) = scene else {
        return None;
    };
    // The §5.37 override owns the box when it claims the leaf, so it owns the
    // baseline too — asking parley here would align against a box the engine
    // did not produce.
    if let Some(measured) = text_measure
        .and_then(|tm| tm.measure_text(&t.content, &t.style, &t.runs, None, t.caret_bearing))
    {
        return measured.baseline;
    }
    let layout = cache.layout_with_runs(&t.content, &t.style, &t.runs, None);
    layout.lines().next().map(|line| line.metrics().baseline)
}

/// R1641 §5.21 — one row whose children align on a common baseline.
///
/// Recorded at build time rather than derived later because it holds the two
/// facts a later pass cannot recover: each child's DECLARED margin (so a
/// re-computed offset replaces the previous one instead of accumulating on top
/// of it), and the baseline of every child that could report one before layout.
struct BaselineRow {
    items: Vec<BaselineItem>,
}

struct BaselineItem {
    node: NodeId,
    /// The application's own `margin.y` / `margin.h`, untouched.
    declared_top: u32,
    declared_bottom: u32,
    /// Where this child's first baseline is, when it is a text leaf and could
    /// be measured without laying anything out. `None` means the baseline has
    /// to be SYNTHESIZED from the laid-out box — see [`baseline_pass`].
    measured: Option<f32>,
}

/// R1641 §5.21 — set each item's top margin so the row's baselines coincide.
/// Returns whether any margin actually moved.
///
/// The offset is injected as taffy `margin.top` BEFORE `compute_layout`, so the
/// row's own height is the layout engine's answer about the shifted children
/// rather than a number this would otherwise have to correct afterwards — and
/// so a parent sizing itself around this row sees the true height on the same
/// pass.
///
/// `synthesize` is what makes the second call different from the first. A child
/// that is not a text leaf has no baseline of its own, and CSS synthesizes one
/// from its bottom margin edge — a number that does not exist until the child
/// has been laid out. So the first pass runs with `synthesize = false` and only
/// the text baselines, and once the tree has a layout the same function runs
/// again with every baseline known. R1641 shipped only the first half and
/// documented the second as absent; this is that half.
///
/// The largest baseline wins, so every offset is `>= 0` and no child is ever
/// lifted above the row's content box.
#[allow(
    clippy::cast_precision_loss,
    reason = "a declared margin and a laid-out box are small px values; the \
              same cast every other margin in `to_taffy_style` already makes"
)]
fn baseline_pass(row: &BaselineRow, tree: &mut TaffyTree<NodeContext>, synthesize: bool) -> bool {
    // Fewer than two boxes cannot be aligned to each other. CSS reaches the
    // same no-op, and stating it here keeps the common single-child row off
    // the `set_style` path entirely.
    if row.items.len() < 2 {
        return false;
    }

    let baselines: Vec<Option<f32>> = row
        .items
        .iter()
        .map(|item| {
            item.measured.or_else(|| {
                if !synthesize {
                    return None;
                }
                // CSS synthesizes a missing baseline at the BOTTOM MARGIN
                // EDGE, which is what makes a box sit on the same line as the
                // text beside it rather than hanging below it.
                let layout = tree.layout(item.node).ok()?;
                Some(layout.size.height + item.declared_bottom as f32)
            })
        })
        .collect();
    if baselines.iter().filter(|b| b.is_some()).count() < 2 {
        return false;
    }
    let deepest = baselines
        .iter()
        .flatten()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);

    let mut moved = false;
    for (item, baseline) in row.items.iter().zip(&baselines) {
        // Rounded because every rect this feeds is snapped to integer pixels
        // downstream; a sub-pixel offset would only be truncated there, and
        // truncation is what makes two equal baselines land a pixel apart.
        let offset = baseline.map_or(0.0, |b| (deepest - b).round().max(0.0));
        // Always written from the DECLARED margin, never added to whatever the
        // previous pass left: that is what keeps a second pass a correction
        // rather than an accumulation.
        let want = item.declared_top as f32 + offset;
        let mut style = tree
            .style(item.node)
            .expect("taffy style query failed")
            .clone();
        let already = style.margin.top;
        style.margin.top = length(want);
        if style.margin.top != already {
            moved = true;
            tree.set_style(item.node, style)
                .expect("taffy set_style failed");
        }
    }
    moved
}

fn build(
    scene: &Scene,
    tree: &mut TaffyTree<NodeContext>,
    cache: &mut LayoutCache,
    text_measure: Option<&dyn TextMeasure>,
    rows: &mut Vec<BaselineRow>,
) -> LayoutShadow {
    // R55.G.4 §5.45 — Scroll's `layout` field now carries its
    // taffy style (seeded with `viewport.{w,h}` by
    // `ScrollNode::new`); the pre-R55.G.4 build-site size override
    // is retired in favour of the unified `layout_style_of` path.
    let style = to_taffy_style(layout_style_of(scene));
    let children = match scene {
        Scene::Container(c) => {
            let shadows: Vec<LayoutShadow> = c
                .children
                .iter()
                .map(|s| build(s, tree, cache, text_measure, rows))
                .collect();
            // R1641 — baseline alignment is a CROSS-axis rule, so it applies
            // only where the cross axis is vertical. `to_taffy_style` maps
            // every direction but `Column` onto a row, and this mirrors that
            // so the two cannot disagree about which container is a row.
            if matches!(c.layout.align_items, AlignItems::Baseline)
                && !matches!(c.layout.flex_direction, FlexDirection::Column)
            {
                rows.push(BaselineRow {
                    items: c
                        .children
                        .iter()
                        .zip(&shadows)
                        .map(|(child, shadow)| {
                            let margin = layout_style_of(child).margin;
                            BaselineItem {
                                node: shadow.node,
                                declared_top: margin.y,
                                declared_bottom: margin.h,
                                measured: text_baseline_of(child, cache, text_measure),
                            }
                        })
                        .collect(),
                });
            }
            shadows
        }
        _ => Vec::new(),
    };
    let child_ids: Vec<NodeId> = children.iter().map(|c| c.node).collect();
    let node = if !child_ids.is_empty() {
        tree.new_with_children(style, &child_ids)
            .expect("taffy new_with_children failed")
    } else if let Scene::Text(t) = scene {
        // R47.4 §5.36 — Text leaves carry the parley measure context
        // so the closure passed to `compute_layout_with_measure` can
        // resolve their intrinsic size. Clone is unavoidable: the
        // closure FnMut bound + node-context ownership semantics keep
        // the context alive across the whole layout pass, beyond the
        // `&Scene` borrow this `build` recurses with.
        tree.new_leaf_with_context(
            style,
            NodeContext::Text {
                content: t.content.clone(),
                style: t.style.clone(),
                runs: t.runs.clone(),
                caret_bearing: t.caret_bearing,
            },
        )
        .expect("taffy new_leaf_with_context failed")
    } else {
        tree.new_leaf(style).expect("taffy new_leaf failed")
    };
    LayoutShadow { node, children }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn apply(
    scene: &mut Scene,
    shadow: &LayoutShadow,
    tree: &TaffyTree<NodeContext>,
    measured: &MeasuredText,
    parent_x: f32,
    parent_y: f32,
) {
    let layout = tree.layout(shadow.node).expect("taffy layout query failed");
    let abs_x = parent_x + layout.location.x;
    let abs_y = parent_y + layout.location.y;
    let rect = Rect::new(
        abs_x.max(0.0) as u32,
        abs_y.max(0.0) as u32,
        layout.size.width.max(0.0) as u32,
        layout.size.height.max(0.0) as u32,
    );
    let _ = assign_rect(scene, rect);

    // R51.1 §5.12 — Text leaves carry their measured line count from
    // the side-channel populated during `compute_layout_with_measure`.
    // Other variants stay at the `TextNode::default()` `line_count = 0`
    // because the field is Text-only by semantics.
    if let Scene::Text(t) = scene {
        t.line_count = measured.lines.get(&shadow.node).copied().unwrap_or(0);
        // Falls back to the laid-out width rather than 0: a node the measure
        // never saw advanced exactly as far as its box, and reporting 0 would
        // make `advance_px - rect.w` read as a NEGATIVE trailing space.
        t.advance_px = measured
            .advances
            .get(&shadow.node)
            .copied()
            .unwrap_or(rect.w);
    }

    if let Scene::Container(c) = scene {
        for (child, shadow_child) in c.children.iter_mut().zip(&shadow.children) {
            apply(child, shadow_child, tree, measured, abs_x, abs_y);
        }
    }
}

fn layout_style_of(scene: &Scene) -> &LayoutStyle {
    static FALLBACK: LayoutStyle = LayoutStyle::new();
    match scene {
        Scene::Box(n) => &n.layout,
        Scene::Text(n) => &n.layout,
        Scene::Path(n) => &n.layout,
        Scene::Image(n) => &n.layout,
        Scene::Container(n) => &n.layout,
        Scene::External(n) => &n.layout,
        // R55.G.4 §5.45 — Scroll's `layout` is seeded with the clip
        // window size by `ScrollNode::new`, so taffy treats it as a
        // fixed-size leaf by default; callers that want `flex_grow`
        // / `margin` / parent-flex participation chain
        // `with_layout(...)`.
        Scene::Scroll(n) => &n.layout,
        // R681 §2 #4 atomic 1 — immediate-mode subtree participates
        // in the §5.21 taffy flex pass via its layout sidecar (parent
        // flex / box parent can size the viewport via `with_size` /
        // `flex_grow` / `padding` / etc.). The post-layout rect feeds
        // back into `ImmediateModeNode::viewport` via `assign_rect`
        // below, which the per-window paint cycle then hands to the
        // Vello backend bridge as the immediate-mode paint area.
        Scene::ImmediateModeNode(n) => &n.layout,
        // R972 §5.41 — the text-grid scaffold participates in the §5.21
        // taffy pass via its layout sidecar; the resolved rect feeds
        // `TextGridNode::rect` (via `assign_rect`), from which the grid
        // derives its `(cols, rows)` winsize dimensions.
        Scene::TextGrid(n) => &n.layout,
        // Effect + future non-exhaustive variants default to identity
        // layout (block, auto sizing). They participate in the flex
        // tree as zero-size leaves until a follow-up slice opts them
        // in explicitly.
        _ => &FALLBACK,
    }
}

/// Apply a rect to whichever variant carries one. Returns `false`
/// for variants without a `rect` field (Effect today; future
/// `non_exhaustive` additions) so the caller can skip them cleanly.
///
/// R55.G.4 §5.45 — `Scene::Scroll` writes the full layout-derived
/// rect into `viewport`. The pre-R55.G.4 partial write (x/y only)
/// was a side effect of the build-site size override; now that
/// `ScrollNode.layout` carries the size intent, taffy's output is
/// the authoritative dimensions and writing the full rect keeps
/// the substrate honest when the caller opts into `flex_grow` or
/// any other layout-driven resize.
fn assign_rect(scene: &mut Scene, rect: Rect) -> bool {
    match scene {
        Scene::Box(BoxNode { rect: r, .. })
        | Scene::Text(TextNode { rect: r, .. })
        | Scene::Path(PathNode { rect: r, .. })
        | Scene::Image(ImageNode { rect: r, .. })
        | Scene::Container(ContainerNode { rect: r, .. })
        | Scene::External(ExternalNode { rect: r, .. }) => {
            *r = rect;
            true
        }
        Scene::Scroll(s) => {
            s.viewport = rect;
            true
        }
        // R681 §2 #4 atomic 1 — the taffy-computed rect lands in
        // `ImmediateModeNode::viewport`; the per-window paint cycle
        // hands this rect to the backend bridge as the
        // viewport-local origin + extent the immediate-mode driver
        // paints into.
        Scene::ImmediateModeNode(n) => {
            n.viewport = rect;
            true
        }
        // R972 §5.41 — the taffy-computed rect lands in
        // `TextGridNode::rect`; `TextGridNode::cols` / `rows` derive the
        // grid's winsize `(cols, rows)` from it (R969 layout-derived).
        Scene::TextGrid(n) => {
            n.rect = rect;
            true
        }
        _ => false,
    }
}

/// (R1560 §5.21) Lower one [`GridTrack`] to taffy's `minmax(min, max)` pair.
///
/// The pairs are CSS's own definitions rather than a choice made here: a
/// `<length>` track is `minmax(len, len)`, an `<flex>` track is
/// `minmax(auto, Nfr)` (which is why a `1fr` column still refuses to shrink
/// below its content), and the intrinsic keywords are the same function on
/// both sides.
#[allow(clippy::cast_precision_loss)]
fn to_taffy_track(track: GridTrack) -> TrackSizingFunction {
    let (min, max) = match track {
        GridTrack::Px(px) => {
            let len = LengthPercentage::from_length(px as f32);
            (
                MinTrackSizingFunction::Fixed(len),
                MaxTrackSizingFunction::Fixed(len),
            )
        }
        GridTrack::Percent(fraction) => {
            let len = LengthPercentage::from_percent(fraction);
            (
                MinTrackSizingFunction::Fixed(len),
                MaxTrackSizingFunction::Fixed(len),
            )
        }
        GridTrack::Fr(units) => (
            MinTrackSizingFunction::Auto,
            MaxTrackSizingFunction::Fraction(units),
        ),
        GridTrack::MinContent => (
            MinTrackSizingFunction::MinContent,
            MaxTrackSizingFunction::MinContent,
        ),
        GridTrack::MaxContent => (
            MinTrackSizingFunction::MaxContent,
            MaxTrackSizingFunction::MaxContent,
        ),
        // `Auto` and any future arm: taffy's `auto` on both sides, which is
        // the sizing every implicit track already gets.
        _ => (MinTrackSizingFunction::Auto, MaxTrackSizingFunction::Auto),
    };
    TrackSizingFunction::Single(MinMax { min, max })
}

/// (R1560 §5.21) Lower a [`GridPlacement`] to taffy's `(start, end)` line pair.
///
/// CSS states one axis of a grid item's placement as a start plus a span, and
/// taffy models that as two independent line values, so the translation is
/// where the two spellings meet: a placed item is `Line(n) / span k`, an
/// unplaced one that still knows its size is `span k / auto`, and no
/// declaration at all is `auto / auto` — taffy's default, so an item that
/// declares nothing lowers exactly as it did before this round.
fn to_taffy_placement(placement: Option<GridPlacement>) -> TaffyLine<TaffyGridPlacement> {
    let Some(placement) = placement else {
        return TaffyLine {
            start: TaffyGridPlacement::Auto,
            end: TaffyGridPlacement::Auto,
        };
    };
    // A zero-track item is not expressible in CSS and dropping it would be a
    // silent hole in the grid, so it covers one track. `text_table` never
    // emits one — this is the lowering refusing to have an undefined case.
    let span = placement.span.max(1);
    match placement.start_line {
        Some(line) => TaffyLine {
            start: TaffyGridPlacement::from_line_index(i16::try_from(line).unwrap_or(i16::MAX)),
            end: TaffyGridPlacement::Span(span),
        },
        None => TaffyLine {
            start: TaffyGridPlacement::Span(span),
            end: TaffyGridPlacement::Auto,
        },
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::field_reassign_with_default,
    clippy::match_same_arms
)]
fn to_taffy_style(layout: &LayoutStyle) -> TaffyStyle {
    let mut s = TaffyStyle::default();
    s.display = match layout.display {
        Display::Block => TaffyDisplay::Block,
        Display::Flex => TaffyDisplay::Flex,
        // (R1560 §5.21) CSS Grid. The tracks below are only read in this mode;
        // taffy ignores them for a flex or block container exactly as CSS
        // does, so a stray declaration cannot change a non-grid layout.
        Display::Grid => TaffyDisplay::Grid,
        _ => TaffyDisplay::Block,
    };
    // (R1560 §5.21) Explicit grid tracks. Empty vectors — every non-grid node
    // in the tree — lower to taffy's own empty default, so the layout graph is
    // bit-identical for every pre-R1560 binding.
    if !layout.grid_template_columns.is_empty() {
        s.grid_template_columns = layout
            .grid_template_columns
            .iter()
            .copied()
            .map(to_taffy_track)
            .collect();
    }
    if !layout.grid_template_rows.is_empty() {
        s.grid_template_rows = layout
            .grid_template_rows
            .iter()
            .copied()
            .map(to_taffy_track)
            .collect();
    }
    s.grid_row = to_taffy_placement(layout.grid_row);
    s.grid_column = to_taffy_placement(layout.grid_column);
    s.flex_direction = match layout.flex_direction {
        FlexDirection::Row => TaffyFlexDir::Row,
        FlexDirection::Column => TaffyFlexDir::Column,
        _ => TaffyFlexDir::Row,
    };
    s.justify_content = Some(match layout.justify_content {
        JustifyContent::Start => TaffyJustify::Start,
        JustifyContent::Center => TaffyJustify::Center,
        JustifyContent::End => TaffyJustify::End,
        JustifyContent::SpaceBetween => TaffyJustify::SpaceBetween,
        JustifyContent::SpaceAround => TaffyJustify::SpaceAround,
        _ => TaffyJustify::Start,
    });
    s.align_items = Some(match layout.align_items {
        AlignItems::Stretch => TaffyAlign::Stretch,
        AlignItems::Start => TaffyAlign::Start,
        AlignItems::Center => TaffyAlign::Center,
        AlignItems::End => TaffyAlign::End,
        // R1641 — `AlignItems::Baseline` lowers to taffy's START, and the
        // shift is pinion's own (see `baseline_pass`).
        //
        // NOT `TaffyAlign::Baseline`, which would look like the obvious
        // mapping and would be a no-op alias for `End` on exactly the items
        // this exists for. taffy takes a leaf's size through a measure
        // function typed `FnOnce(..) -> Size<f32>` — there is no channel for a
        // baseline — so every measured leaf returns `first_baselines:
        // Point::NONE`, and flexbox then does `baseline.unwrap_or(height)`.
        // For a text leaf that synthesized baseline IS the box's bottom edge.
        // Shipping the one-line mapping would have published a variant that
        // moves nothing, which is the error direction that inflates a
        // capability silently.
        AlignItems::Baseline => TaffyAlign::Start,
        _ => TaffyAlign::Stretch,
    });
    s.gap = TaffySize {
        width: length(layout.gap as f32),
        height: length(layout.gap as f32),
    };
    s.size = TaffySize {
        width: to_dimension(layout.size.width),
        height: to_dimension(layout.size.height),
    };
    // (R1086 §5.21) `LayoutStyle::min_size` lowering. The `Size::auto()`
    // default lowers to `Dimension::Auto` per axis = taffy's struct
    // default `min_size`, so the layout graph stays bit-identical for
    // every pre-R1086 binding. A `SizeValue::Px(0)` axis overrides taffy's
    // CSS automatic flex minimum to zero, letting a flex child shrink
    // below its content (the `flex-basis: 0; flex-grow: r; min: 0` idiom
    // `view_splitter` uses for its ratio children).
    s.min_size = TaffySize {
        width: to_dimension(layout.min_size.width),
        height: to_dimension(layout.min_size.height),
    };
    // (R1685 §5.21 §5.45) `LayoutStyle::overflow` — the layout half of the one
    // declaration whose paint half is the clip `Scene::clip_window` publishes.
    // taffy has carried this property all along; not lowering it is what left
    // a consumer spelling `overflow: hidden`'s layout effect by hand as
    // `min_size: Px(0)` and then finding no way to say the paint half at all.
    //
    // `Overflow::Hidden` zeroes the CSS automatic minimum size on both axes,
    // which is the same relaxation `min_size: Px(0)` produces and the reason
    // this pair is one word. Both axes take the same value because the field
    // is one (see `Overflow`: a mixed clipping/visible box is not expressible
    // in CSS either). `Visible` is taffy's struct default, so every pre-R1685
    // binding lowers bit-identically.
    s.overflow = TaffyPoint {
        x: to_taffy_overflow(layout.overflow),
        y: to_taffy_overflow(layout.overflow),
    };
    s.flex_grow = layout.flex_grow;
    // (R1536 §5.21) `flex-shrink`. Defaults to `1.0` on both sides, so every
    // pre-R1536 binding lowers bit-identically.
    s.flex_shrink = layout.flex_shrink;
    // (R684 §5.21) `LayoutStyle::flex_basis` lowering. `None` maps to
    // `Dimension::Auto` (taffy's default — intrinsic content drives
    // the basis); `Some(v)` reuses `to_dimension` so `SizeValue::Px`,
    // `SizeValue::Percent`, and `SizeValue::Auto` all reach taffy via
    // the canonical translation. The pre-R684 substrate had no
    // `flex_basis` field — taffy's struct default of `Auto` carried
    // every binding's behaviour, so `None → Auto` keeps the layout
    // graph bit-identical.
    if let Some(basis) = layout.flex_basis {
        s.flex_basis = to_dimension(basis);
    }
    // §5.21 R24 slice 4: Rect-as-4-inset (x=left, y=top, w=right,
    // h=bottom) → taffy Rect<LengthPercentage>. taffy's padding /
    // margin both take pixel lengths.
    s.padding = TaffyRect {
        left: LengthPercentage::from_length(layout.padding.x as f32),
        right: LengthPercentage::from_length(layout.padding.w as f32),
        top: LengthPercentage::from_length(layout.padding.y as f32),
        bottom: LengthPercentage::from_length(layout.padding.h as f32),
    };
    s.margin = TaffyRect {
        left: length(layout.margin.x as f32),
        right: length(layout.margin.w as f32),
        top: length(layout.margin.y as f32),
        bottom: length(layout.margin.h as f32),
    };
    // (R55.D.6 §5.45 §5.21) Absolute positioning override. When the
    // application sets `absolute_position`, taffy lifts the child out
    // of its parent's flex / block flow and resolves its position via
    // the `inset.{left, top}` pair — mirroring CSS
    // `position: absolute; left: <x>; top: <y>`. The unspecified
    // `right` / `bottom` insets stay `Auto` so the declared `size`
    // alone defines the box dimensions (CSS resolves `width`/`height`
    // against `auto` insets in the same direction). Default `None`
    // keeps `position: Relative` so the entire pre-R55.D.6 layout
    // graph stays bit-identical.
    if let Some((left, top)) = layout.absolute_position {
        s.position = TaffyPosition::Absolute;
        s.inset = TaffyRect {
            left: LengthPercentageAuto::from_length(left as f32),
            right: LengthPercentageAuto::Auto,
            top: LengthPercentageAuto::from_length(top as f32),
            bottom: LengthPercentageAuto::Auto,
        };
    }
    s
}

/// (R1685 §5.21) Lower one [`Overflow`] to taffy's.
///
/// Two variants map to two, and the pair taffy has that this does not —
/// `Clip` and `Scroll` — is a decision recorded on [`Overflow`], not an
/// omission: `Scroll` would be a second scrolling model beside
/// [`Scene::Scroll`], and taffy's `Clip` keeps a content-based minimum where
/// CSS relaxes it, so publishing it would publish an engine quirk.
fn to_taffy_overflow(value: Overflow) -> TaffyOverflow {
    // Derived from the same predicate the renderers read, not from a second
    // list of variants. That is the whole thesis of R1685 applied to its own
    // lowering: a declaration whose halves are matched separately is a
    // declaration whose halves can disagree, and this file is where the layout
    // half is decided.
    //
    // The day `Overflow` gains a variant that clips WITHOUT relaxing the
    // automatic minimum (CSS `clip` as taffy reads it), this becomes two
    // questions and takes the second predicate — see `Overflow::clips`.
    if value.clips() {
        TaffyOverflow::Hidden
    } else {
        TaffyOverflow::Visible
    }
}

#[allow(clippy::cast_precision_loss, clippy::match_same_arms)]
fn to_dimension(value: SizeValue) -> Dimension {
    match value {
        SizeValue::Auto => auto(),
        SizeValue::Px(n) => length(n as f32),
        SizeValue::Percent(p) => percent(f32::from(p) / 100.0),
        _ => auto(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::StubExternal;
    use pinion_core::scene::ExternalNode;
    use pinion_core::style::{Color, FlexDirection, JustifyContent, Size, TextStyle};

    fn cache() -> LayoutCache {
        LayoutCache::new()
    }

    // ── R1538 §5.16 §5.45 — the layout pass's node census ──

    /// Count `Scene` nodes independently of the layout, so the census is
    /// checked against a second observation rather than against itself.
    fn scene_node_count(scene: &Scene) -> u32 {
        match scene {
            Scene::Container(c) => c.children.iter().map(scene_node_count).sum::<u32>() + 1,
            Scene::Scroll(s) => scene_node_count(s.content.as_ref()) + 1,
            _ => 1,
        }
    }

    fn text_leaf(s: &str) -> Scene {
        Scene::Text(TextNode::new(
            s,
            pinion_core::scene::Rect::new(0, 0, 40, 16),
        ))
    }

    #[test]
    fn r1538_census_counts_every_node_the_pass_measured() {
        // The claim is "one layout node per scene node", and the only way to
        // check it is to count the scene a second way. An assertion against a
        // number the same walk produced would pass however the walk is wrong.
        let mut scene = Scene::Container(ContainerNode::new(vec![
            text_leaf("a"),
            Scene::Container(ContainerNode::new(vec![text_leaf("b"), text_leaf("c")])),
            text_leaf("d"),
        ]));
        let expected = scene_node_count(&scene);
        let pass = compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 320, 200);
        assert_eq!(expected, 6, "fixture shape pinned, so a drift is visible");
        assert_eq!(pass.nodes, expected);
    }

    #[test]
    fn r1538_census_descends_into_scrolled_content() {
        // `build` stops at a `Scene::Scroll`, so its content gets an
        // independent taffy pass whose count has no outer equivalent. A census
        // that took only the outer tree's would report a flat number for
        // exactly the case a large scene lives in — a million rows inside one
        // scroll — which is the reading it exists to prevent.
        let rows: Vec<Scene> = (0..12).map(|i| text_leaf(&format!("row {i}"))).collect();
        let content = Scene::Container(ContainerNode::new(rows));
        let mut scrolled = Scene::Container(ContainerNode::new(vec![Scene::Scroll(
            pinion_core::scene::ScrollNode::new(
                pinion_core::scene::Rect::new(0, 0, 320, 200),
                content,
            ),
        )]));
        let mut flat = Scene::Container(ContainerNode::new(vec![text_leaf("only")]));

        let with_scroll = compute_layout_with_scroll_dirty(&mut scrolled, &mut cache(), 320, 200);
        let without = compute_layout_with_scroll_dirty(&mut flat, &mut cache(), 320, 200);

        assert_eq!(with_scroll.nodes, scene_node_count(&scrolled));
        assert!(
            with_scroll.nodes > without.nodes + 12,
            "scrolled rows must be counted: {} vs {}",
            with_scroll.nodes,
            without.nodes,
        );
    }

    #[test]
    fn r1538_census_grows_with_the_tree_and_not_with_the_viewport() {
        // The census must track the thing it claims to track. Doubling the
        // node count doubles it; doubling the viewport does not — which is the
        // asymmetry a scale guard reads, and it would be invisible if the
        // number were a function of the window.
        let small = || {
            Scene::Container(ContainerNode::new(
                (0..8).map(|i| text_leaf(&format!("{i}"))).collect(),
            ))
        };
        let large = || {
            Scene::Container(ContainerNode::new(
                (0..16).map(|i| text_leaf(&format!("{i}"))).collect(),
            ))
        };

        let (mut a, mut b, mut wide) = (small(), large(), small());
        let a = compute_layout_with_scroll_dirty(&mut a, &mut cache(), 320, 200).nodes;
        let b = compute_layout_with_scroll_dirty(&mut b, &mut cache(), 320, 200).nodes;
        let wide = compute_layout_with_scroll_dirty(&mut wide, &mut cache(), 1280, 800).nodes;

        assert_eq!(a, 9);
        assert_eq!(b, 17);
        assert_eq!(
            a, wide,
            "the census counts nodes, so a four-times-larger viewport must not move it",
        );
    }

    #[test]
    fn r1538_census_is_independent_of_the_dirty_bit() {
        // Two facts on one value must not be derivable from each other. A
        // scene that settles clean and one that does not are laid out here at
        // the same size, so a census wired to the dirty bit would show it.
        let mut clean = Scene::Container(ContainerNode::new(vec![text_leaf("x")]));
        let first = compute_layout_with_scroll_dirty(&mut clean, &mut cache(), 320, 200);
        let second = compute_layout_with_scroll_dirty(&mut clean, &mut cache(), 320, 200);
        assert_eq!(
            first.nodes, second.nodes,
            "same tree, same count, whatever the bit did",
        );
        assert!(
            !second.scroll_dirty,
            "a re-pass over a settled plain tree is clean"
        );
    }

    #[test]
    fn block_root_fills_viewport() {
        // A Container with Display::Block (default) and Auto size
        // expands to the viewport bounds taffy was given.
        let mut scene = Scene::Container(ContainerNode::new(vec![]));
        compute_layout(&mut scene, &mut cache(), 320, 200);
        let Scene::Container(c) = &scene else {
            panic!("expected container")
        };
        assert_eq!(c.rect.w, 320);
        assert_eq!(c.rect.h, 200);
    }

    #[test]
    fn harvest_measures_laid_out_row_heights_through_the_real_layout_pass() {
        // R1199 — the crux integration test the R1194 round lacked: drive the
        // ACTUAL layout pass over a measured-list-shaped scene and assert the
        // harvest read each row's taffy-laid-out `rect.h` back into the
        // MeasuredRowState (not fed directly, as the unit tests do). Three rows
        // whose fixed-height content forces distinct natural heights 30/40/50;
        // width-fixed / height-auto slots tagged `measured-row:<i>`, inside a
        // ScrollNode wired with BOTH a ScrollState and a MeasuredRowState.
        use pinion_core::scene::ScrollNode;
        use pinion_core::widgets::measured_rows::{MeasuredRowState, measured_row_tag};
        use pinion_core::widgets::scroll::ScrollState;
        use std::rc::Rc;

        let measured = Rc::new(MeasuredRowState::new(3, 20)); // estimate 20/row
        let scroll = Rc::new(ScrollState::new());
        let row_heights = [30u32, 40, 50];
        let slots: Vec<Scene> = row_heights
            .iter()
            .enumerate()
            .map(|(i, &h)| {
                // Row content: a fixed-height box → the height-auto slot resolves
                // its own height to `h` (the natural-content-height the harvest reads).
                let row = Scene::Box(
                    BoxNode::filled(Rect::default(), Color::default())
                        .with_layout(LayoutStyle::new().with_size(Size::px(100, h))),
                );
                Scene::Container(
                    ContainerNode::new(vec![row])
                        .with_tag(measured_row_tag(None, i))
                        .with_layout(
                            LayoutStyle::new()
                                .with_absolute_position(0, 0)
                                .with_size(Size::width_px(200)),
                        ),
                )
            })
            .collect();
        let sizer = Scene::Container(
            ContainerNode::new(slots).with_layout(LayoutStyle::new().with_size(Size::px(200, 300))),
        );
        let content = Scene::Container(ContainerNode::new(vec![sizer]));
        let mut scene = Scene::Scroll(
            ScrollNode::from_state(Rc::clone(&scroll), Rect::new(0, 0, 200, 150), content)
                .with_measured_rows(Rc::clone(&measured)),
        );

        assert_eq!(
            measured.measured_count(),
            0,
            "nothing measured before layout"
        );
        compute_layout(&mut scene, &mut cache(), 200, 150);
        // The harvest read each slot's laid-out rect.h back into the state.
        assert_eq!(
            measured.measured_count(),
            3,
            "the layout pass measured all three rows via the harvest round-trip",
        );
        assert_eq!(measured.measured_height(0), Some(30));
        assert_eq!(measured.measured_height(1), Some(40));
        assert_eq!(measured.measured_height(2), Some(50));
    }

    #[test]
    fn flex_row_centers_single_fixed_child() {
        // Container = Flex Row, justify_content=Center, align_items=Center.
        // Child = fixed 160x80. Expected center position in 320x200
        // viewport: (80, 60).
        let child = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default())
                .with_layout(LayoutStyle::new().with_size(Size::px(160, 80))),
        );
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center);
        let mut scene = Scene::Container(ContainerNode::new(vec![child]).with_layout(layout));
        compute_layout(&mut scene, &mut cache(), 320, 200);
        let Scene::Container(c) = &scene else {
            panic!("expected container")
        };
        assert_eq!(c.rect.w, 320);
        assert_eq!(c.rect.h, 200);
        let Scene::Box(b) = &c.children[0] else {
            panic!("expected box child")
        };
        assert_eq!(b.rect.w, 160);
        assert_eq!(b.rect.h, 80);
        assert_eq!(b.rect.x, 80);
        assert_eq!(b.rect.y, 60);
    }

    #[test]
    fn flex_column_distributes_two_children() {
        // Column flex with two leaves of fixed height; gap=10.
        // Expected: first child at y=0, second at y=80+10=90.
        let layout = LayoutStyle::new().flex(FlexDirection::Column).with_gap(10);
        let leaf = |h: u32| {
            Scene::Box(
                BoxNode::filled(Rect::default(), Color::default())
                    .with_layout(LayoutStyle::new().with_size(Size::px(100, h))),
            )
        };
        let mut scene =
            Scene::Container(ContainerNode::new(vec![leaf(80), leaf(60)]).with_layout(layout));
        compute_layout(&mut scene, &mut cache(), 200, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(a) = &c.children[0] else {
            panic!("first child")
        };
        let Scene::Box(b) = &c.children[1] else {
            panic!("second child")
        };
        assert_eq!(a.rect.y, 0);
        assert_eq!(a.rect.h, 80);
        assert_eq!(b.rect.y, 90);
        assert_eq!(b.rect.h, 60);
    }

    // (R1086 §5.21) A flex child whose content is an 800px-tall box —
    // intrinsic min-content (800) is larger than its 0.5 ratio share of a
    // 600px viewport (300). `flex_basis:0 + flex_grow:0.5` alone is NOT
    // enough: taffy's CSS automatic flex minimum pins the child to its
    // content (800) so both children overflow. `min_size.height = Px(0)`
    // overrides that automatic minimum, letting the child distribute by
    // ratio. Mirrors sprag's vertical-reorganize-keeps-both-panels guard.
    fn big_content_child(flex_grow: f32, min_zero: bool) -> Scene {
        let inner = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default())
                .with_layout(LayoutStyle::new().with_size(Size::px(100, 800))),
        );
        let mut layout = LayoutStyle::new()
            .with_flex_basis(SizeValue::Px(0))
            .with_flex_grow(flex_grow);
        if min_zero {
            layout = layout.with_min_size(Size::auto().with_height(SizeValue::Px(0)));
        }
        Scene::Container(ContainerNode::new(vec![inner]).with_layout(layout))
    }

    #[test]
    fn r1086_min_size_height_zero_lets_large_content_flex_child_distribute_by_ratio() {
        let parent = LayoutStyle::new().flex(FlexDirection::Column);
        let mut scene = Scene::Container(
            ContainerNode::new(vec![
                big_content_child(0.5, true),
                big_content_child(0.5, true),
            ])
            .with_layout(parent),
        );
        compute_layout(&mut scene, &mut cache(), 960, 600);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Container(a) = &c.children[0] else {
            panic!("first child")
        };
        let Scene::Container(b) = &c.children[1] else {
            panic!("second child")
        };
        // Each child shrinks to its 0.5 * 600 = 300 ratio share (±2 for
        // taffy f32→u32 rounding), NOT its 800px content.
        assert!(
            a.rect.h.abs_diff(300) <= 2,
            "first child h={} (~300)",
            a.rect.h
        );
        assert!(
            b.rect.h.abs_diff(300) <= 2,
            "second child h={} (~300)",
            b.rect.h
        );
        // Both panels stay within the 600px viewport (the acceptance bar).
        assert_eq!(a.rect.y, 0);
        assert!(
            b.rect.y + b.rect.h <= 600,
            "second panel must stay on-screen: y={} h={}",
            b.rect.y,
            b.rect.h,
        );
    }

    #[test]
    fn r1086_without_min_override_large_content_flex_child_clamps_and_overflows() {
        // Regression witness: with min_size left at the `Auto` default,
        // taffy's automatic flex minimum clamps each child to its 800px
        // content → the second panel lands off-screen (the bug PR-30
        // fixes). This pins the mechanism so a taffy upgrade that silently
        // changed the automatic-minimum behaviour would surface here.
        let parent = LayoutStyle::new().flex(FlexDirection::Column);
        let mut scene = Scene::Container(
            ContainerNode::new(vec![
                big_content_child(0.5, false),
                big_content_child(0.5, false),
            ])
            .with_layout(parent),
        );
        compute_layout(&mut scene, &mut cache(), 960, 600);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Container(b) = &c.children[1] else {
            panic!("second child")
        };
        // Clamped to 800px content → second panel starts at y=800, fully
        // off the 600px viewport.
        assert!(
            b.rect.y >= 600,
            "without min:0 the second panel overflows off-screen, got y={}",
            b.rect.y,
        );
    }

    /// ★★★ R1685 — the layout half of `overflow: hidden`, from the
    /// declaration instead of from arithmetic that has the same effect.
    ///
    /// This is [`r1086_min_size_height_zero_lets_large_content_flex_child_distribute_by_ratio`]
    /// with one word changed: `Overflow::Hidden` in place of
    /// `min_size.height = Px(0)`. Both children must land on the same numbers,
    /// because CSS's automatic minimum size is what both are overriding — and
    /// the pair of tests is the proof that a consumer who says what they MEAN
    /// gets what the hand-spelled minimum gave them.
    ///
    /// The witness below it is the same relation the R1086 pair has: with the
    /// declaration left at its `Visible` default the second panel walks off
    /// the viewport, so this test cannot pass by the layout being insensitive
    /// to the field.
    #[test]
    fn r1685_overflow_hidden_relaxes_the_automatic_minimum_like_min_size_zero() {
        let ratio_child = |hidden: bool| {
            let inner = Scene::Box(
                BoxNode::filled(Rect::default(), Color::default())
                    .with_layout(LayoutStyle::new().with_size(Size::px(100, 800))),
            );
            let mut layout = LayoutStyle::new()
                .with_flex_basis(SizeValue::Px(0))
                .with_flex_grow(0.5);
            if hidden {
                layout = layout.with_overflow(Overflow::Hidden);
            }
            Scene::Container(ContainerNode::new(vec![inner]).with_layout(layout))
        };
        let lay_out = |children: Vec<Scene>| {
            let mut scene = Scene::Container(
                ContainerNode::new(children)
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            );
            compute_layout(&mut scene, &mut cache(), 960, 600);
            let Scene::Container(c) = &scene else {
                panic!("container")
            };
            c.children
                .iter()
                .map(pinion_core::scene::Scene::rect)
                .collect::<Vec<_>>()
        };

        let declared = lay_out(vec![ratio_child(true), ratio_child(true)]);
        let by_hand = lay_out(vec![
            big_content_child(0.5, true),
            big_content_child(0.5, true),
        ]);
        assert_eq!(
            declared, by_hand,
            "`Overflow::Hidden` and `min_size: Px(0)` override the same CSS \
             rule, so they must produce the same layout"
        );
        assert!(
            declared[1].y + declared[1].h <= 600,
            "and that layout is the one that fits: {declared:?}"
        );

        // The witness: nothing here is insensitive to the declaration.
        let visible = lay_out(vec![ratio_child(false), ratio_child(false)]);
        assert!(
            visible[1].y >= 600,
            "with `Visible` the automatic minimum pins each child at its 800px \
             content and the second panel leaves the viewport: {visible:?}"
        );
    }

    #[test]
    fn container_padding_offsets_child_origin() {
        // R24 slice 4: LayoutStyle.padding feeds taffy padding;
        // child rect.{x,y} shifts by the parent's left+top padding.
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_padding(pinion_core::scene::Rect::new(10, 20, 10, 20));
        let child = Scene::Box(
            BoxNode::filled(pinion_core::scene::Rect::default(), Color::default())
                .with_layout(LayoutStyle::new().with_size(Size::px(50, 30))),
        );
        let mut scene = Scene::Container(ContainerNode::new(vec![child]).with_layout(layout));
        compute_layout(&mut scene, &mut cache(), 200, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(b) = &c.children[0] else {
            panic!("box child")
        };
        // padding.x (left=10), padding.y (top=20) shift the child.
        assert_eq!(b.rect.x, 10);
        assert_eq!(b.rect.y, 20);
    }

    #[test]
    fn external_node_participates_with_explicit_size() {
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center);
        let ext = Scene::External(
            ExternalNode::new(Box::new(StubExternal::new()))
                .with_layout(LayoutStyle::new().with_size(Size::px(64, 32))),
        );
        let mut scene = Scene::Container(ContainerNode::new(vec![ext]).with_layout(layout));
        compute_layout(&mut scene, &mut cache(), 200, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::External(e) = &c.children[0] else {
            panic!("external")
        };
        assert_eq!(e.rect.w, 64);
        assert_eq!(e.rect.h, 32);
        assert_eq!(e.rect.x, 68); // (200 - 64) / 2
        assert_eq!(e.rect.y, 84); // (200 - 32) / 2
    }

    #[test]
    fn text_leaf_intrinsic_measure_drives_flex_center() {
        // R47.4 §5.36 — Scene::Text leaf with no explicit Size resolves
        // its width/height through parley measure (LayoutCache) and
        // participates in flex Center/Center as a non-zero box. Without
        // the MeasureFunc wire the leaf was 0×0 → a "centered" single
        // point at viewport mid; the user-visible bug R47.3 left open.
        let text = Scene::Text(TextNode::styled(
            "Click me!",
            Rect::default(),
            TextStyle::new().with_size_px(18),
        ));
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center);
        let mut scene = Scene::Container(ContainerNode::new(vec![text]).with_layout(layout));
        let mut c = cache();
        compute_layout(&mut scene, &mut c, 320, 200);
        let Scene::Container(container) = &scene else {
            panic!("expected container")
        };
        let Scene::Text(t) = &container.children[0] else {
            panic!("expected text child")
        };
        assert!(t.rect.w > 0, "text leaf width should be parley-measured");
        assert!(t.rect.h > 0, "text leaf height should be parley-measured");
        // R51.1 §5.12 — measured line count is populated alongside the
        // rect. A short single-word label in a 320-wide viewport must
        // resolve to a single line regardless of system font fallback.
        assert_eq!(
            t.line_count, 1,
            "single-word label in 320-wide viewport must be 1 line"
        );
        // flex Center → child rect.x ≈ (320 - w) / 2 and rect.y ≈
        // (200 - h) / 2. Exact pixel depends on the system font width;
        // we assert the offsets are non-trivial (not 0 = left/top edge).
        assert!(
            t.rect.x > 0,
            "Center flex must shift text right of x=0 (got x={})",
            t.rect.x
        );
        assert!(
            t.rect.y > 0,
            "Center flex must shift text below y=0 (got y={})",
            t.rect.y
        );
    }

    #[test]
    fn text_line_count_zero_before_layout() {
        // R51.1 §5.12 — `TextNode::styled` / `TextNode::new` default
        // `line_count = 0`. The measure pass populates it; readers
        // can rely on `0` meaning "no shape pass has run yet" as a
        // sentinel distinct from any valid measured count.
        let t = TextNode::styled(
            "Click me!",
            Rect::default(),
            TextStyle::new().with_size_px(18),
        );
        assert_eq!(t.line_count, 0);
    }

    #[test]
    fn text_line_count_stable_across_adjacent_viewport_widths() {
        // R47.7.6 / R51.1 §5.12 — sub-pixel parley widths get ceil'd
        // before they reach taffy, so adjacent integer viewport widths
        // (the per-frame sequence during mouse-drag resize) produce
        // the same `line_count`. Missing the `ceil` would let
        // `cache.layout(...).width()` return e.g. 77.8 while taffy's
        // child slot is 77 — parley would then break to a second
        // line on every other frame, jittering `line_count` between
        // 1 and 2 across the drag.
        let label = "Click me!";
        let style = TextStyle::new().with_size_px(18);
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center);
        let mut cache = LayoutCache::new();
        let mut counts = Vec::with_capacity(21);
        // 300..=320 — a 21-wide window straddles the natural label
        // width on every reasonable system font, exercising the
        // adjacent-width path that produced the original R47.7.6
        // jitter on mouse-drag resize.
        for w in 300_u32..=320 {
            let text = Scene::Text(TextNode::styled(label, Rect::default(), style.clone()));
            let mut scene =
                Scene::Container(ContainerNode::new(vec![text]).with_layout(layout.clone()));
            compute_layout(&mut scene, &mut cache, w, 200);
            let Scene::Container(container) = &scene else {
                panic!("container");
            };
            let Scene::Text(t) = &container.children[0] else {
                panic!("text child");
            };
            counts.push(t.line_count);
        }
        assert!(
            counts.iter().all(|&n| n == 1),
            "line_count must stay 1 across adjacent widths 300..=320 (got {counts:?})"
        );
    }

    #[test]
    fn text_line_count_increases_when_max_width_forces_wrap() {
        // R51.1 §5.12 — when the available width is genuinely narrower
        // than the natural text width, parley wraps and `line_count`
        // grows accordingly. This bounds the ceil-stability test
        // above: the surface really does report >1 lines when the
        // content truly does not fit, so the AI client can rely on
        // `line_count > 1` as a real wrap signal.
        let content = "The quick brown fox jumps over the lazy dog";
        let style = TextStyle::new().with_size_px(18);
        let text = Scene::Text(
            TextNode::styled(content, Rect::default(), style)
                .with_layout(LayoutStyle::new().with_size(pinion_core::style::Size::px(60, 200))),
        );
        let layout = LayoutStyle::new().flex(FlexDirection::Row);
        let mut scene = Scene::Container(ContainerNode::new(vec![text]).with_layout(layout));
        let mut cache = LayoutCache::new();
        compute_layout(&mut scene, &mut cache, 320, 200);
        let Scene::Container(container) = &scene else {
            panic!("container");
        };
        let Scene::Text(t) = &container.children[0] else {
            panic!("text child");
        };
        assert!(
            t.line_count >= 2,
            "60px-wide slot must force the sentence to wrap (got {})",
            t.line_count
        );
    }

    mod r55_g2 {
        //! R55.G.2 §5.45 — `compute_layout` descends into
        //! `Scene::Scroll.content`. Content rects come out in
        //! scroll-local coordinates with `MaxContent` on the main
        //! axis, so a flex Column overflows the clip window naturally
        //! instead of being shrunk to fit.

        use super::*;
        use pinion_core::scene::{ContainerNode, Rect, ScrollAxis, ScrollNode};
        use pinion_core::style::{Color, FlexDirection, JustifyContent, LayoutStyle, Size};

        fn fixed_row(h: u32) -> Scene {
            fixed_row_w(220, h)
        }

        fn fixed_row_w(w: u32, h: u32) -> Scene {
            Scene::Container(
                ContainerNode::new(vec![])
                    .with_layout(LayoutStyle::new().with_size(Size::px(w, h))),
            )
        }

        #[test]
        fn scroll_content_flex_column_lays_out_row_y_positions() {
            // Content = flex Column with gap=6, 3 rows of fixed 220×28.
            // Expected: row[0]@(0,0), row[1]@(0,34), row[2]@(0,68).
            let content_layout = LayoutStyle::new().flex(FlexDirection::Column).with_gap(6);
            let content = Scene::Container(
                ContainerNode::new(vec![fixed_row(28), fixed_row(28), fixed_row(28)])
                    .with_layout(content_layout),
            );
            let scroll = ScrollNode::new(Rect::new(70, 78, 220, 164), content);
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else {
                panic!("outer")
            };
            let Scene::Scroll(s) = &outer.children[0] else {
                panic!("scroll")
            };
            // R55.G.3 §5.45 — viewport's w/h stay app-set (clip
            // window intent); viewport's x/y are layout-derived.
            // Block-display outer places Scroll at (0, 0).
            assert_eq!(s.viewport.w, 220);
            assert_eq!(s.viewport.h, 164);
            assert_eq!(s.viewport.x, 0);
            assert_eq!(s.viewport.y, 0);
            // Content rects are scroll-local (origin at (0, 0)).
            let Scene::Container(c) = s.content.as_ref() else {
                panic!("content")
            };
            let Scene::Container(r0) = &c.children[0] else {
                panic!("row0")
            };
            let Scene::Container(r1) = &c.children[1] else {
                panic!("row1")
            };
            let Scene::Container(r2) = &c.children[2] else {
                panic!("row2")
            };
            assert_eq!(r0.rect, Rect::new(0, 0, 220, 28));
            assert_eq!(r1.rect, Rect::new(0, 34, 220, 28));
            assert_eq!(r2.rect, Rect::new(0, 68, 220, 28));
        }

        #[test]
        fn r55_g5_layout_writes_scroll_max_from_content_height() {
            // R55.G.5 §5.45 — after `compute_layout`, the attached
            // `ScrollState`'s max_y reflects the actual laid-out
            // content height minus the clip viewport. Pre-R55.G.5
            // every view fn duplicated this arithmetic + called
            // `set_max` manually; now the layout pass writes it.
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            // 12 rows × 28 + 11 × 6 gap = 402 intrinsic height; the
            // viewport is 164 tall, so the expected max_y = 238.
            let rows: Vec<Scene> = (0..12).map(|_| fixed_row(28)).collect();
            let content = Scene::Container(
                ContainerNode::new(rows)
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(6)),
            );
            let state = Rc::new(ScrollState::new());
            // Sanity: bound starts at the `ScrollState::new` default
            // (0) so the test catches a real write, not a no-op.
            assert_eq!(state.max(), (0, 0));
            let scroll =
                ScrollNode::new(Rect::new(0, 0, 220, 164), content).with_state(Rc::clone(&state));
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let (max_x, max_y) = state.max();
            assert_eq!(max_x, 0, "content fits horizontally, no x overflow");
            assert_eq!(max_y, 238, "max_y = content_h(402) - viewport_h(164)");
        }

        #[test]
        fn r57_x_scrollbar_first_layout_returns_scroll_dirty_true() {
            // R57.X.scrollbar §5.45 — the very first
            // `compute_layout_with_scroll_dirty` against a fresh
            // ScrollState whose `max == (0, 0)` MUST return `true`
            // because the layout-derived `max_y = 238` is not equal
            // to the pre-write `0`. The shell substrate uses this
            // bit to re-run `V::view` + `compute_layout` on the same
            // paint cycle so the scrollbar widget never paints a
            // stale full-track thumb to the user.
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            let rows: Vec<Scene> = (0..12).map(|_| fixed_row(28)).collect();
            let content = Scene::Container(
                ContainerNode::new(rows)
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(6)),
            );
            let state = Rc::new(ScrollState::new());
            assert_eq!(state.max(), (0, 0), "pre-condition: fresh ScrollState");
            let scroll =
                ScrollNode::new(Rect::new(0, 0, 220, 164), content).with_state(Rc::clone(&state));
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            let dirty =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320).scroll_dirty;
            assert!(
                dirty,
                "first-paint pass must report dirty (max moved 0 -> 238)",
            );
            assert_eq!(state.max(), (0, 238), "max bound written by the same pass");
        }

        #[test]
        fn r784_horizontal_scroll_unbounds_width_and_writes_max_x() {
            // R784 §5.45 — a `ScrollAxis::Horizontal` container lets
            // its content overflow the viewport WIDTH: the layout pass
            // unbounds the width axis so a flex Row of fixed-width
            // cells summing wider than the clip window keeps its
            // intrinsic width (instead of being shrunk to fit), and
            // `update_scroll_state_bounds` writes the overflow into
            // `max_x`. The height stays clamped to the viewport, so
            // `max_y` is 0 — the frozen-header data-grid relies on this
            // (its outer horizontal scroll must never scroll
            // vertically, or the header would slide off the top).
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            // 6 cells × 120 = 720 content width inside a 300-wide clip.
            let cells: Vec<Scene> = (0..6).map(|_| fixed_row_w(120, 40)).collect();
            let content = Scene::Container(
                ContainerNode::new(cells).with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
            );
            let state = Rc::new(ScrollState::new());
            let scroll = ScrollNode::new(Rect::new(0, 0, 300, 40), content)
                .with_axis(ScrollAxis::Horizontal)
                .with_state(Rc::clone(&state));
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            // 720 content − 300 viewport = 420 horizontal overflow;
            // the 40-tall row fits the 40-tall viewport, so no vertical.
            assert_eq!(
                state.max(),
                (420, 0),
                "horizontal overflow flows into max_x; height clamped to the viewport",
            );
        }

        #[test]
        fn r877_both_axis_scroll_unbounds_both_and_writes_both_maxima() {
            // R877 §5.45 — a `ScrollAxis::Both` container (the pannable
            // 2-D canvas) keeps its content's declared extent on BOTH
            // axes: a fixed 720×600 world inside a 300×200 clip writes
            // (420, 400) maxima — the R784 single-axis modes each
            // clamp the cross axis, so neither could host a canvas
            // panned freely in x and y.
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            let content = fixed_row_w(720, 600);
            let state = Rc::new(ScrollState::new());
            let scroll = ScrollNode::new(Rect::new(0, 0, 300, 200), content)
                .with_axis(ScrollAxis::Both)
                .with_state(Rc::clone(&state));
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            assert_eq!(
                state.max(),
                (420, 400),
                "both axes overflow into (max_x, max_y)",
            );
        }

        #[test]
        fn r784_vertical_scroll_default_still_clamps_width() {
            // R784 regression guard — the default `ScrollAxis::Vertical`
            // (no `with_axis`) reproduces the pre-R784 behaviour
            // exactly: the content root is clamped to `viewport.w`, so a
            // Row wider than the clip writes `max_x == 0` (no horizontal
            // overflow). Protects every existing vertical scroll
            // consumer from the new width-unbounded branch.
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            let cells: Vec<Scene> = (0..6).map(|_| fixed_row_w(120, 40)).collect();
            let content = Scene::Container(
                ContainerNode::new(cells).with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
            );
            let state = Rc::new(ScrollState::new());
            let scroll =
                ScrollNode::new(Rect::new(0, 0, 300, 40), content).with_state(Rc::clone(&state));
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            assert_eq!(
                state.max().0,
                0,
                "default vertical scroll clamps width to the viewport (pre-R784 behaviour)",
            );
        }

        #[test]
        fn r859_follower_scroll_skips_publish_so_settles_dirty_false() {
            // R859 §5.45 — the real frozen-column-grid configuration: two
            // *vertical* body scrolls share one vertical `ScrollState` (so
            // they scroll in vertical lockstep) but sit in side-by-side
            // columns of different cross-axis *widths* (frozen pane 300 vs
            // scrolling pane 200). If both published, `set_measured_viewport`
            // would flip-flop the shared `measured_w` and report dirty=true
            // every pass forever. The follower flag suppresses the frozen
            // pane's write, so the pair settles and a second pass reports
            // dirty=false. The primary still owns `max_y` + the published
            // measured viewport.
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            fn v_col(w: u32) -> Scene {
                // A tall column: 6 × 120 = 720 content height, `w` wide.
                let cells: Vec<Scene> = (0..6).map(|_| fixed_row_w(w, 120)).collect();
                Scene::Container(
                    ContainerNode::new(cells)
                        .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
                )
            }

            let state = Rc::new(ScrollState::new());
            // Primary: the 300-wide scrolling-pane body owns the bounds write.
            let primary =
                ScrollNode::new(Rect::new(0, 0, 300, 40), v_col(300)).with_state(Rc::clone(&state));
            // Follower: the 200-wide frozen-pane body slides with the same
            // state but never publishes (different cross-axis width).
            let follower = ScrollNode::new(Rect::new(0, 0, 200, 40), v_col(200))
                .with_state(Rc::clone(&state))
                .as_follower();
            let mut scene = Scene::Container(ContainerNode::new(vec![
                Scene::Scroll(follower),
                Scene::Scroll(primary),
            ]));

            let dirty_first =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320).scroll_dirty;
            assert!(dirty_first, "prime pass writes the primary's bounds");
            let dirty_second =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320).scroll_dirty;
            assert!(
                !dirty_second,
                "follower suppresses its publish, so the pair settles (no perpetual re-pass)",
            );
            // 720 content − 40 viewport = 680 vertical overflow, written by
            // the primary; the width is clamped so max_x is 0.
            assert_eq!(state.max(), (0, 680), "primary owns max_y; width clamped");
            assert_eq!(
                state.measured_viewport(),
                (300, 40),
                "published measured viewport is the PRIMARY's (300 wide), not the follower's 200-wide pane",
            );
        }

        #[test]
        fn r859_two_primaries_sharing_state_oscillate_negative_control() {
            // R859 negative control — proves the bug the follower flag
            // cures. Two *primary* vertical scrolls sharing one state with
            // different cross-axis viewport *widths* both publish
            // `set_measured_viewport`, so the shared `measured_w` flip-flops
            // (300 ⇄ 200) every pass and the dirty bit never settles — the
            // perpetual re-pass marking the frozen pane a follower eliminates.
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            fn v_col(w: u32) -> Scene {
                let cells: Vec<Scene> = (0..6).map(|_| fixed_row_w(w, 120)).collect();
                Scene::Container(
                    ContainerNode::new(cells)
                        .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
                )
            }

            let state = Rc::new(ScrollState::new());
            let a =
                ScrollNode::new(Rect::new(0, 0, 300, 40), v_col(300)).with_state(Rc::clone(&state));
            let b =
                ScrollNode::new(Rect::new(0, 0, 200, 40), v_col(200)).with_state(Rc::clone(&state));
            let mut scene =
                Scene::Container(ContainerNode::new(vec![Scene::Scroll(a), Scene::Scroll(b)]));

            let _ = compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320);
            let dirty_second =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320).scroll_dirty;
            assert!(
                dirty_second,
                "two primaries with mismatched widths oscillate measured_w → never settles",
            );
        }

        #[test]
        fn r57_x_scrollbar_second_layout_returns_scroll_dirty_false() {
            // R57.X.scrollbar §5.45 — once the ScrollState's max has
            // settled to the layout-derived value, a second
            // `compute_layout_with_scroll_dirty` on the same scene
            // MUST return `false`. The Signal equality-skip path in
            // `ScrollState::set_max` floors the per-axis Signal
            // revision so the dirty bit stays at `false`, and the
            // substrate's re-run guard short-circuits — zero
            // overhead on every steady-state frame.
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            let rows: Vec<Scene> = (0..12).map(|_| fixed_row(28)).collect();
            let content = Scene::Container(
                ContainerNode::new(rows)
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(6)),
            );
            let state = Rc::new(ScrollState::new());
            let scroll =
                ScrollNode::new(Rect::new(0, 0, 220, 164), content).with_state(Rc::clone(&state));
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            // Prime: first pass writes max, returns dirty=true.
            let dirty_first =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320).scroll_dirty;
            assert!(dirty_first, "prime pass must report dirty");
            // Steady state: second pass on the same scene at the
            // same viewport sees an unchanged max bound, no Signal
            // revision advance, dirty=false.
            let dirty_second =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320).scroll_dirty;
            assert!(
                !dirty_second,
                "second pass on a settled ScrollState must report dirty=false (Signal equality-skip)",
            );
        }

        #[test]
        fn r57_x_scrollbar_scroll_dirty_false_when_no_state_attached() {
            // R57.X.scrollbar §5.45 — a Scroll with no ScrollState
            // attached cannot mutate any max bound, so the dirty bit
            // stays at `false`. Protects against a future
            // refactor that accidentally returns `true` for the
            // no-state branch (which would force every frame to
            // re-run the view+layout pair pointlessly).
            let rows: Vec<Scene> = (0..12).map(|_| fixed_row(28)).collect();
            let content = Scene::Container(
                ContainerNode::new(rows)
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(6)),
            );
            let scroll = ScrollNode::new(Rect::new(0, 0, 220, 164), content);
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            let dirty =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320).scroll_dirty;
            assert!(
                !dirty,
                "Scroll without state attached has no Signals to touch — dirty must be false",
            );
        }

        #[test]
        fn r55_g4_scroll_with_flex_grow_stretches_in_parent_flex() {
            // R55.G.4 §5.45 — `with_layout` overrides the default
            // `Size::px(viewport.{w,h})` so the Scroll can opt into
            // `flex_grow` and fill the remaining cross-axis space.
            // Proves the layout sidecar plumbing reaches Scroll, not
            // just the size override that the R55.G.3 hack baked in.
            let content = Scene::Container(ContainerNode::new(vec![]));
            let scroll = ScrollNode::new(Rect::new(0, 0, 100, 50), content)
                .with_layout(LayoutStyle::new().with_flex_grow(1.0));
            let outer_layout = LayoutStyle::new().flex(FlexDirection::Row);
            let mut scene = Scene::Container(
                ContainerNode::new(vec![Scene::Scroll(scroll)]).with_layout(outer_layout),
            );

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else {
                panic!("outer")
            };
            let Scene::Scroll(s) = &outer.children[0] else {
                panic!("scroll")
            };
            // Scroll grew to the full 360-wide row instead of staying
            // at the 100-wide default `viewport` width — proves the
            // taffy size came from `layout.flex_grow`, not the
            // pre-R55.G.4 unconditional viewport override.
            assert_eq!(s.viewport.w, 360, "flex_grow stretched viewport.w");
        }

        #[test]
        fn r55_g3_scroll_centered_via_outer_flex_writes_viewport_position() {
            // Outer Container = flex Row + JustifyContent::Center +
            // AlignItems::Center. Scroll inside is 220×164 inside a
            // 360×320 viewport — expected centred at
            // ((360-220)/2, (320-164)/2) = (70, 78). Proves R55.G.3
            // routes Scroll through parent flex.
            let content_layout = LayoutStyle::new().flex(FlexDirection::Column).with_gap(6);
            let content = Scene::Container(
                ContainerNode::new(vec![fixed_row(28)]).with_layout(content_layout),
            );
            let scroll = ScrollNode::new(Rect::new(0, 0, 220, 164), content);
            let outer_layout = LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center);
            let mut scene = Scene::Container(
                ContainerNode::new(vec![Scene::Scroll(scroll)]).with_layout(outer_layout),
            );

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else {
                panic!("outer")
            };
            let Scene::Scroll(s) = &outer.children[0] else {
                panic!("scroll")
            };
            assert_eq!(s.viewport.x, 70, "viewport.x layout-derived");
            assert_eq!(s.viewport.y, 78, "viewport.y layout-derived");
            assert_eq!(s.viewport.w, 220, "viewport.w app-set");
            assert_eq!(s.viewport.h, 164, "viewport.h app-set");
        }

        #[test]
        fn scroll_content_total_height_can_exceed_viewport() {
            // 12 rows × 28 + 11 × 6 gap = 402 > 164 viewport. With
            // `MaxContent` on the main axis the flex column lays
            // children at their natural heights instead of shrinking.
            let rows: Vec<Scene> = (0..12).map(|_| fixed_row(28)).collect();
            let content = Scene::Container(
                ContainerNode::new(rows)
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(6)),
            );
            let scroll = ScrollNode::new(Rect::new(0, 0, 220, 164), content);
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else {
                panic!("outer")
            };
            let Scene::Scroll(s) = &outer.children[0] else {
                panic!("scroll")
            };
            let Scene::Container(c) = s.content.as_ref() else {
                panic!("content")
            };
            // Last row's y = 11 × (28 + 6) = 374, well past the
            // 164-tall viewport — proves flex did not compress.
            let Scene::Container(last) = c.children.last().unwrap() else {
                panic!()
            };
            assert_eq!(last.rect.y, 374);
            assert_eq!(last.rect.h, 28);
        }

        #[test]
        fn scroll_content_cross_axis_bounded_by_viewport_width() {
            // Content child without explicit width inherits viewport.w
            // (220) as the cross-axis bound under flex Column.
            let stretchy = Scene::Container(
                ContainerNode::new(vec![Scene::Box(
                    BoxNode::filled(Rect::default(), Color::default())
                        .with_layout(LayoutStyle::new().with_size(Size::px(40, 20))),
                )])
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center),
                ),
            );
            let content = Scene::Container(
                ContainerNode::new(vec![stretchy])
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            );
            let scroll = ScrollNode::new(Rect::new(0, 0, 220, 80), content);
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else {
                panic!("outer")
            };
            let Scene::Scroll(s) = &outer.children[0] else {
                panic!("scroll")
            };
            let Scene::Container(c) = s.content.as_ref() else {
                panic!("content")
            };
            let Scene::Container(stretchy) = &c.children[0] else {
                panic!("stretchy")
            };
            // Stretched to the viewport.w cross-axis bound.
            assert_eq!(stretchy.rect.w, 220);
            let Scene::Box(b) = &stretchy.children[0] else {
                panic!("box")
            };
            // Centered inside the 220-wide stretchy row.
            assert_eq!(b.rect.x, 90);
            assert_eq!(b.rect.w, 40);
        }

        #[test]
        fn nested_scroll_content_recurses_through_lay_out_scroll_contents() {
            // Outer Scroll content contains another Scroll, whose
            // own content must also be laid out by the recursive
            // pass — proves the lay_out_scroll_contents tail call
            // descends into nested scrolls.
            let inner_content = Scene::Container(
                ContainerNode::new(vec![fixed_row_w(200, 40)])
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            );
            let inner_scroll =
                Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 200, 60), inner_content));
            let outer_content = Scene::Container(
                ContainerNode::new(vec![inner_scroll])
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            );
            let outer_scroll =
                Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 220, 160), outer_content));
            let mut scene = Scene::Container(ContainerNode::new(vec![outer_scroll]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(root) = &scene else {
                panic!("root")
            };
            let Scene::Scroll(outer) = &root.children[0] else {
                panic!("outer scroll")
            };
            let Scene::Container(outer_c) = outer.content.as_ref() else {
                panic!("outer content")
            };
            let Scene::Scroll(inner) = &outer_c.children[0] else {
                panic!("inner scroll")
            };
            let Scene::Container(inner_c) = inner.content.as_ref() else {
                panic!("inner content")
            };
            // Inner content's row was laid out by the nested-scroll
            // recursive pass — rect is non-zero.
            let Scene::Container(row) = &inner_c.children[0] else {
                panic!("inner row")
            };
            assert_eq!(row.rect, Rect::new(0, 0, 200, 40));
        }

        // ─────────────────────────────────────────────────────────────
        // R1445 §5.45 §5.27 — the deferred tail pin, driven through the
        // REAL pass. The consumer's whole contribution is the arming;
        // every number below is one the layout pass produced.
        // ─────────────────────────────────────────────────────────────

        #[test]
        fn r1445_deferred_pin_lands_on_the_bound_this_pass_measured() {
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            // 12 rows of 28 + 11 gaps of 6 = 402 content in a 164 viewport.
            // The binding never states any of that — it arms, and the pass
            // resolves 402 - 164 = 238.
            let rows: Vec<Scene> = (0..12).map(|_| fixed_row(28)).collect();
            let content = Scene::Container(
                ContainerNode::new(rows)
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(6)),
            );
            let state = Rc::new(ScrollState::new());
            state.follow_measured_tail();
            let scroll =
                ScrollNode::new(Rect::new(0, 0, 220, 164), content).with_state(Rc::clone(&state));
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            let dirty =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320).scroll_dirty;

            assert_eq!(state.max(), (0, 238), "the pass measured the extent");
            assert_eq!(state.offset(), (0, 238), "and the pin rode that bound");
            assert!(
                dirty,
                "the frame must re-run: the scene was laid out against the pre-pin offset",
            );
            // R1458 — the arming outlives the pass that moved the offset,
            // because that pass's own bound may not be the last word (the
            // windowed case). It is spent by the settled pass the `dirty` bit
            // above already schedules; here the second pass finds the same
            // bound, moves nothing, and clears it.
            assert!(state.is_following_measured_tail(), "still converging");
            let settled =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache(), 360, 320).scroll_dirty;
            assert!(
                !settled,
                "the second pass moves nothing: this frame settled"
            );
            assert_eq!(state.offset(), (0, 238), "and stayed at the tail");
            assert!(!state.is_following_measured_tail(), "one-shot, spent");
        }

        #[test]
        fn r1445_a_pass_with_no_arming_leaves_the_reader_where_they_are() {
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            // Same scene, no arming: a reader parked mid-document stays put
            // across a pass that re-affirms the same bound. This is the
            // negative control for the test above — without it, a pin that
            // fired unconditionally would pass that one just as well.
            let rows: Vec<Scene> = (0..12).map(|_| fixed_row(28)).collect();
            let content = Scene::Container(
                ContainerNode::new(rows)
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(6)),
            );
            let state = Rc::new(ScrollState::new());
            state.set_max(0, 238);
            state.scroll_to(0, 60);
            let scroll =
                ScrollNode::new(Rect::new(0, 0, 220, 164), content).with_state(Rc::clone(&state));
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            let mut cache = cache();
            // The first pass is dirty for a reason of its own — it publishes
            // the measured viewport (R774) — so the steady-state claim is
            // made against the second.
            let _ = compute_layout_with_scroll_dirty(&mut scene, &mut cache, 360, 320);
            assert_eq!(state.offset(), (0, 60), "unmoved by the first pass");
            let dirty =
                compute_layout_with_scroll_dirty(&mut scene, &mut cache, 360, 320).scroll_dirty;

            assert_eq!(state.offset(), (0, 60), "still unmoved");
            assert!(!dirty, "steady-state frame stays clean: nothing armed it");
        }

        #[test]
        fn r1445_pin_observes_the_harvest_refined_bound_not_the_pre_harvest_one() {
            // Why the pin is its own walk *after* `harvest_measured_rows`
            // rather than a branch inside `update_scroll_state_bounds`: a
            // measured list's spacer is sized from the ESTIMATE (3 × 20 = 60,
            // which fits the 100-tall viewport → bound 0), and only the
            // harvest's measured total (30+40+50 = 120 → bound 20) is the
            // truth. Pinning from the bounds pass would land at 0 — a whole
            // viewport short of the tail the consumer asked for.
            use pinion_core::scene::ScrollNode;
            use pinion_core::widgets::measured_rows::{MeasuredRowState, measured_row_tag};
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            let measured = Rc::new(MeasuredRowState::new(3, 20));
            let scroll = Rc::new(ScrollState::new());
            let slots: Vec<Scene> = [30u32, 40, 50]
                .iter()
                .enumerate()
                .map(|(i, &h)| {
                    let row = Scene::Container(
                        ContainerNode::new(vec![])
                            .with_layout(LayoutStyle::new().with_size(Size::px(100, h))),
                    );
                    Scene::Container(
                        ContainerNode::new(vec![row])
                            .with_tag(measured_row_tag(None, i))
                            .with_layout(
                                LayoutStyle::new()
                                    .with_absolute_position(0, 0)
                                    .with_size(Size::width_px(200)),
                            ),
                    )
                })
                .collect();
            // The spacer carries the pre-harvest ESTIMATE (3 × 20).
            let sizer = Scene::Container(
                ContainerNode::new(slots)
                    .with_layout(LayoutStyle::new().with_size(Size::px(200, 60))),
            );
            let content = Scene::Container(ContainerNode::new(vec![sizer]));
            scroll.follow_measured_tail();
            let mut scene = Scene::Scroll(
                ScrollNode::from_state(Rc::clone(&scroll), Rect::new(0, 0, 200, 100), content)
                    .with_measured_rows(Rc::clone(&measured)),
            );

            compute_layout(&mut scene, &mut cache(), 200, 100);

            assert_eq!(measured.total_height(), 120, "harvest measured 30+40+50");
            assert_eq!(
                scroll.max(),
                (0, 20),
                "harvest's refined bound is the last word"
            );
            assert_eq!(
                scroll.offset(),
                (0, 20),
                "the pin rode the refined bound, not the estimate-derived 0",
            );
        }

        #[test]
        fn r1445_nested_scroll_arming_fires_from_the_inner_node() {
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            // The walk recurses into `Scroll.content`, so a scroll nested
            // inside another scroll pins against its own measured bound.
            let inner_state = Rc::new(ScrollState::new());
            inner_state.follow_measured_tail();
            let inner_content = Scene::Container(
                ContainerNode::new(vec![fixed_row_w(200, 300)])
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            );
            let inner = Scene::Scroll(
                ScrollNode::new(Rect::new(0, 0, 200, 60), inner_content)
                    .with_state(Rc::clone(&inner_state)),
            );
            let outer_content = Scene::Container(
                ContainerNode::new(vec![inner])
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            );
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(
                ScrollNode::new(Rect::new(0, 0, 220, 160), outer_content),
            )]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            assert_eq!(inner_state.max(), (0, 240), "300 content - 60 viewport");
            assert_eq!(inner_state.offset(), (0, 240), "inner pin fired");
        }

        #[test]
        fn r1445_horizontal_scroll_pins_the_axis_that_actually_overflows() {
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            // A horizontal strip that grew to the right lands at its right-hand
            // tail. NOTE what this does *not* prove: the layout pass clamps a
            // single-axis scroll's cross extent to the viewport, so `max_y` is
            // 0 here by construction and "pin x only" and "pin both maxima"
            // agree. The axis branch is discriminated by
            // `scroll::tests::r1445_axis_declares_which_edge_is_the_tail`
            // (hand-set cross bound) and by the follower test below (a shared
            // state whose publisher declares a different axis).
            let cells: Vec<Scene> = (0..6).map(|_| fixed_row_w(120, 40)).collect();
            let content = Scene::Container(
                ContainerNode::new(cells).with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
            );
            let state = Rc::new(ScrollState::new());
            state.follow_measured_tail();
            let scroll = ScrollNode::new(Rect::new(0, 0, 300, 40), content)
                .with_axis(ScrollAxis::Horizontal)
                .with_state(Rc::clone(&state));
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            assert_eq!(state.max(), (420, 0), "720 content - 300 viewport");
            assert_eq!(state.offset(), (420, 0), "pinned to the right-hand tail");
        }

        #[test]
        fn r1445_nested_scroll_bound_is_written_by_a_single_compute_layout() {
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            // Regression guard for the `fold_scrolls` lift: one pass writes the
            // bounds of BOTH scrolls, including on a pass where the outer's own
            // bound moves. (Written while hunting a short-circuit bug in the
            // pre-lift `dirty || recurse(content)` — there is none: the inner
            // scroll is reached through `lay_out_scroll_contents`'s own
            // `compute_layout_inner` pass. The test is kept because THAT path
            // is what it actually pins, and nothing else pinned it.)
            let inner_state = Rc::new(ScrollState::new());
            let outer_state = Rc::new(ScrollState::new());
            let inner = Scene::Scroll(
                ScrollNode::new(
                    Rect::new(0, 0, 200, 60),
                    Scene::Container(ContainerNode::new(vec![fixed_row_w(200, 300)])),
                )
                .with_state(Rc::clone(&inner_state)),
            );
            // A tall sibling makes the OUTER bound move on this pass — the
            // condition a short-circuiting descent would have been sensitive to.
            let outer_content = Scene::Container(
                ContainerNode::new(vec![inner, fixed_row_w(200, 400)])
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            );
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(
                ScrollNode::new(Rect::new(0, 0, 220, 160), outer_content)
                    .with_state(Rc::clone(&outer_state)),
            )]));

            // ONE pass — no re-pass to paper over a skipped visit.
            compute_layout(&mut scene, &mut cache(), 360, 320);

            assert_eq!(
                outer_state.max(),
                (0, 300),
                "the outer bound moved, which is the precondition",
            );
            assert_eq!(
                inner_state.max(),
                (0, 240),
                "the nested scroll's bound was written by that same pass",
            );
        }

        #[test]
        fn r1445_pin_rides_the_bound_publisher_not_whichever_node_comes_first() {
            use pinion_core::widgets::scroll::ScrollState;
            use std::rc::Rc;

            // R859 linked scroll: a follower shares the primary's state and
            // publishes no bound. It must not fire the pin either — otherwise
            // "which axes are the tail" would be decided by scene order. The
            // fixture puts the follower FIRST and gives it a different axis
            // from the primary, so an implementation that let either node fire
            // lands somewhere else.
            let state = Rc::new(ScrollState::new());
            state.follow_measured_tail();

            // Follower: vertical, listed first, publishes nothing.
            let follower = Scene::Scroll(
                ScrollNode::new(
                    Rect::new(0, 0, 300, 200),
                    Scene::Container(ContainerNode::new(vec![fixed_row_w(720, 600)])),
                )
                .with_axis(ScrollAxis::Vertical)
                .with_state(Rc::clone(&state))
                .as_follower(),
            );
            // Primary: a 2-D canvas — 720×600 content in a 300×200 window.
            let primary = Scene::Scroll(
                ScrollNode::new(
                    Rect::new(0, 0, 300, 200),
                    Scene::Container(ContainerNode::new(vec![fixed_row_w(720, 600)])),
                )
                .with_axis(ScrollAxis::Both)
                .with_state(Rc::clone(&state)),
            );
            let mut scene = Scene::Container(ContainerNode::new(vec![follower, primary]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            assert_eq!(state.max(), (420, 400), "the primary published both bounds");
            assert_eq!(
                state.offset(),
                (420, 400),
                "the pin used the PUBLISHER's axis (Both), not the follower's",
            );
        }

        // ----- R1554 §5.39 — the disabled cascade rides the settle loop -----

        /// A view scene with a declared-disabled region holding one coloured
        /// leaf, over an opaque backdrop.
        fn gated_view() -> Scene {
            use pinion_core::scene::{BoxNode, ContainerNode};
            use pinion_core::style::{BoxStyle, Color, LayoutStyle};
            let leaf = Scene::Box(
                BoxNode::new(
                    pinion_core::scene::Rect::new(0, 0, 10, 10),
                    BoxStyle::filled(Color::rgb(0x10, 0x20, 0x30)),
                )
                .with_tag("leaf"),
            );
            Scene::Container(
                ContainerNode::new(vec![Scene::Container(
                    ContainerNode::new(vec![leaf])
                        .with_tag("region")
                        .with_layout(LayoutStyle::new().with_disabled(true)),
                )])
                .with_style(BoxStyle::filled(Color::rgb(0xff, 0xff, 0xff))),
            )
        }

        fn leaf_fill(scene: &Scene) -> pinion_core::style::Color {
            fn walk(s: &Scene) -> Option<pinion_core::style::Color> {
                if s.tag() == Some("leaf") {
                    if let Scene::Box(b) = s {
                        return Some(b.style.fill);
                    }
                }
                match s {
                    Scene::Container(c) => c.children.iter().find_map(walk),
                    Scene::Scroll(s) => walk(&s.content),
                    _ => None,
                }
            }
            walk(scene).expect("the leaf is in the tree")
        }

        /// The seam claim: EVERY paint-scene producer in both backends funnels
        /// through this loop, so putting the cascade here is what makes a
        /// window and a terminal agree about which controls are inert. Asserted
        /// on the loop rather than on a shell, because the shells are what the
        /// loop exists to keep from drifting.
        #[test]
        fn r1554_the_settle_loop_resolves_the_disabled_cascade() {
            let raw = gated_view();
            assert!(
                !leaf_fill(&raw).eq(&pinion_core::style::Color::rgb(0x10, 0x20, 0x30).lerp(
                    pinion_core::style::Color::rgb(0xff, 0xff, 0xff),
                    pinion_core::widgets::interaction::DISABLED,
                )),
                "the raw view scene is NOT faded — otherwise the assertion \
                 below could not tell the loop apart from the view fn",
            );
            let settled = settle_to_fixed_point(gated_view, |_| false);
            assert_eq!(
                leaf_fill(&settled.scene),
                pinion_core::style::Color::rgb(0x10, 0x20, 0x30).lerp(
                    pinion_core::style::Color::rgb(0xff, 0xff, 0xff),
                    pinion_core::widgets::interaction::DISABLED,
                ),
                "the scene the loop hands back is cascaded",
            );
            assert!(
                settled.scene.is_disabled()
                    || matches!(&settled.scene, Scene::Container(c)
                        if c.children[0].is_disabled()),
                "and the derived flag is written, not only the ink",
            );
        }

        /// Each settle pass re-runs the view, so each pass gets a FRESH,
        /// un-cascaded scene — and the one the loop returns must be cascaded
        /// whichever pass produced it. A cascade applied only before the loop
        /// would leave every re-passed frame unfaded.
        #[test]
        fn r1554_a_re_passed_frame_is_cascaded_too() {
            let mut passes = 0;
            let settled = settle_to_fixed_point(gated_view, |_| {
                passes += 1;
                // Two dirty passes, then clean: the returned scene is the
                // THIRD view run's.
                passes < 3
            });
            assert_eq!(settled.passes, 3, "the loop re-ran the view twice");
            assert_eq!(
                leaf_fill(&settled.scene),
                pinion_core::style::Color::rgb(0x10, 0x20, 0x30).lerp(
                    pinion_core::style::Color::rgb(0xff, 0xff, 0xff),
                    pinion_core::widgets::interaction::DISABLED,
                ),
                "and the scene it kept is cascaded exactly once",
            );
        }

        /// R1458 — the settle loop a shell paint runs: build the view from the
        /// current state, lay it out, and repeat while the pass reports it
        /// moved something. `passes` is capped so a non-converging fixture
        /// fails the assertion rather than the test runner.
        fn settle(
            build: impl Fn() -> Scene,
            cache: &mut LayoutCache,
            w: u32,
            h: u32,
        ) -> (usize, bool) {
            for pass in 1..=8 {
                let mut scene = build();
                if !compute_layout_with_scroll_dirty(&mut scene, cache, w, h).scroll_dirty {
                    return (pass, true);
                }
            }
            (8, false)
        }

        #[test]
        fn r1458_pin_rides_the_bound_the_frame_settled_on() {
            use pinion_core::widgets::measured_rows::{MeasuredRowState, measured_row_tag};
            use pinion_core::widgets::scroll::{ScrollState, max_scroll_offset};
            use pinion_core::widgets::virtual_list::compute_visible_range_variable;
            use std::rc::Rc;

            // A measured list whose LAST row is far taller than the estimate —
            // the shape a transcript of wrapped prose has after an append. The
            // harvest can only measure rows the view materialized, so the bound
            // the FIRST pass publishes is provisional: it still counts the tail
            // row at the estimate. A pin that consumed its arming there lands
            // short by the whole refinement, and the arming is gone before the
            // truth arrives.
            const ROWS: usize = 40;
            const EST: u32 = 20;
            const TAIL_H: u32 = 200;
            const VIEWPORT_H: u32 = 100;
            let height_of = |i: usize| if i == ROWS - 1 { TAIL_H } else { EST };
            let exact_total: u32 = (0..ROWS).map(height_of).sum();

            let measured = Rc::new(MeasuredRowState::new(ROWS, EST));
            let scroll = Rc::new(ScrollState::new());
            scroll.follow_measured_tail();

            // Mirrors `view_measured_list`: window against the measurement
            // table, absolute-position each slot at its table top, and size the
            // spacer to the table total.
            let build = || {
                let offsets = measured.offsets();
                let window =
                    compute_visible_range_variable(scroll.offset_y(), VIEWPORT_H, &offsets, 0);
                let slots: Vec<Scene> = window
                    .indices()
                    .map(|i| {
                        let row = Scene::Container(ContainerNode::new(vec![]).with_layout(
                            LayoutStyle::new().with_size(Size::px(200, height_of(i))),
                        ));
                        Scene::Container(
                            ContainerNode::new(vec![row])
                                .with_tag(measured_row_tag(None, i))
                                .with_layout(
                                    LayoutStyle::new()
                                        .with_absolute_position(0, offsets.row_top(i))
                                        .with_size(Size::width_px(200)),
                                ),
                        )
                    })
                    .collect();
                let sizer = Scene::Container(ContainerNode::new(slots).with_layout(
                    LayoutStyle::new().with_size(Size::px(200, offsets.total_height())),
                ));
                Scene::Scroll(
                    ScrollNode::from_state(
                        Rc::clone(&scroll),
                        Rect::new(0, 0, 200, VIEWPORT_H),
                        Scene::Container(ContainerNode::new(vec![sizer])),
                    )
                    .with_measured_rows(Rc::clone(&measured)),
                )
            };

            let (passes, converged) = settle(build, &mut cache(), 200, VIEWPORT_H);

            assert!(converged, "the frame settled within the pass budget");
            assert!(
                passes > 1,
                "the fixture needs a re-pass, or it proves nothing"
            );
            assert_eq!(
                measured.total_height(),
                exact_total,
                "the settle loop walked the window down to the tail row, so \
                 every row is measured",
            );
            assert_eq!(
                scroll.max(),
                (0, max_scroll_offset(exact_total, VIEWPORT_H)),
                "the settled bound is the measured one",
            );
            assert_eq!(
                scroll.offset(),
                scroll.max(),
                "the pin rode the bound the frame SETTLED on, not the \
                 provisional one the first pass published",
            );
            assert!(
                !scroll.is_following_measured_tail(),
                "and it is spent — a settled pass consumes the arming",
            );
        }
    }

    #[test]
    fn text_leaf_measure_populates_layout_cache() {
        // The measure pass should hit the same LayoutCache subsequent
        // paint passes use; shape work amortizes across measure + paint
        // within one frame.
        let text = Scene::Text(TextNode::styled(
            "Hello",
            Rect::default(),
            TextStyle::new().with_size_px(16),
        ));
        let mut scene = Scene::Container(
            ContainerNode::new(vec![text]).with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center),
            ),
        );
        let mut c = cache();
        assert_eq!(c.len(), 0, "fresh cache is empty");
        compute_layout(&mut scene, &mut c, 320, 200);
        assert!(!c.is_empty(), "measure pass populates LayoutCache");
    }

    /// R1641.4 §5.36 §5.12 — the box declines to count a trailing space, and
    /// now SAYS how much it declined.
    ///
    /// The consumer case verbatim: `"최소 "` beside `"1회"` renders as
    /// `최소1회`, because the first leaf's box excludes the trailing space's
    /// advance. That behaviour is correct and stays; what was missing is that
    /// the amount existed nowhere a client could read, so the only way to
    /// learn it was to look at pixels — the thing §2 #7 exists to avoid.
    #[test]
    fn r1641_4_a_text_node_reports_the_advance_its_box_excluded() {
        let mut c = cache();
        let mut scene = Scene::Container(
            ContainerNode::new(vec![
                Scene::Text(TextNode::styled(
                    "ab ",
                    Rect::default(),
                    TextStyle::new().with_size_px(20),
                )),
                Scene::Text(TextNode::styled(
                    "ab",
                    Rect::default(),
                    TextStyle::new().with_size_px(20),
                )),
            ])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
        );
        compute_layout(&mut scene, &mut c, 400, 200);

        let Scene::Container(container) = &scene else {
            panic!("container")
        };
        let (Scene::Text(spaced), Scene::Text(solid)) =
            (&container.children[0], &container.children[1])
        else {
            panic!("two text leaves")
        };

        assert!(
            spaced.advance_px > spaced.rect.w,
            "the trailing space is in the advance and not in the box: \
             advance={} box={}",
            spaced.advance_px,
            spaced.rect.w,
        );
        assert_eq!(
            spaced.rect.w, solid.rect.w,
            "and the two boxes are the SAME width, which is the symptom the \
             consumer reported",
        );
        assert_eq!(
            solid.advance_px, solid.rect.w,
            "a string with no trailing space reports no excluded advance",
        );
        // The pair is comparable because both are ceiled px. A client reading
        // `advance_px - rect.w` gets the gap it did not get on screen.
        assert!(
            spaced.advance_px - spaced.rect.w >= 1,
            "the difference is a whole pixel a client can act on",
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R1641 §5.21 — `AlignItems::Baseline`.
    //
    // The consumer case verbatim: a 56px numeral with its 20px unit beside
    // it. `End` drops the unit by the numeral's descender, and the workaround
    // is a hand-measured bottom margin that nothing keeps true.
    //
    // Every assertion here is RELATIVE — "these two baselines coincide", not
    // "this baseline is at 45px" — because the face a bare `LayoutCache`
    // resolves is the host's. Each test also asserts the pair it aligns is a
    // pair that STARTS apart, so a fixture that stopped discriminating fails
    // instead of passing.
    // ─────────────────────────────────────────────────────────────────
    mod r1641_baseline_alignment {
        use super::*;
        use pinion_core::style::{AlignItems, BoxStyle};

        const BIG: u32 = 56;
        const SMALL: u32 = 20;

        fn text(content: &str, size: u32) -> Scene {
            Scene::Text(TextNode::styled(
                content,
                Rect::default(),
                TextStyle::new().with_size_px(size),
            ))
        }

        fn row(align: AlignItems, children: Vec<Scene>) -> Scene {
            Scene::Container(
                ContainerNode::new(children).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(align),
                ),
            )
        }

        /// Where a laid-out text leaf's first baseline sits in window space.
        #[allow(
            clippy::cast_precision_loss,
            reason = "a laid-out y within a 200px viewport is exact in f32"
        )]
        fn absolute_baseline(scene: &Scene, cache: &mut LayoutCache) -> f32 {
            let Scene::Text(t) = scene else {
                panic!("not a text leaf")
            };
            let within = cache
                .layout_with_runs(&t.content, &t.style, &t.runs, None)
                .lines()
                .next()
                .expect("a non-empty string lays out at least one line")
                .metrics()
                .baseline;
            t.rect.y as f32 + within
        }

        fn children_of(scene: &Scene) -> &[Scene] {
            let Scene::Container(c) = scene else {
                panic!("not a container")
            };
            &c.children
        }

        #[test]
        fn r1641_a_baseline_row_puts_both_baselines_on_one_line() {
            let mut c = cache();
            let mut aligned = row(
                AlignItems::Baseline,
                vec![text("92.5", BIG), text("kg", SMALL)],
            );
            let mut top = row(
                AlignItems::Start,
                vec![text("92.5", BIG), text("kg", SMALL)],
            );
            compute_layout(&mut aligned, &mut c, 400, 200);
            compute_layout(&mut top, &mut c, 400, 200);

            // Non-vacuity first: the same pair top-aligned has baselines that
            // do NOT meet. Without this the alignment assertion below would
            // also pass on a row where the two sizes happened to agree.
            let untouched = children_of(&top);
            let apart = (absolute_baseline(&untouched[0], &mut c)
                - absolute_baseline(&untouched[1], &mut c))
            .abs();
            assert!(
                apart > 1.0,
                "the fixture must state a difference to remove: {apart}px apart under Start",
            );

            let items = children_of(&aligned);
            let gap =
                (absolute_baseline(&items[0], &mut c) - absolute_baseline(&items[1], &mut c)).abs();
            assert!(
                gap <= 1.0,
                "the baselines coincide (within the integer-pixel snap): {gap}px",
            );
        }

        #[test]
        fn r1641_the_shifted_item_moves_down_and_the_row_still_contains_it() {
            let mut c = cache();
            let mut scene = row(
                AlignItems::Baseline,
                vec![text("92.5", BIG), text("kg", SMALL)],
            );
            compute_layout(&mut scene, &mut c, 400, 200);

            let items = children_of(&scene);
            let (big, small) = (items[0].rect(), items[1].rect());
            assert_eq!(big.y, 0, "the deepest baseline does not move");
            assert!(
                small.y > 0,
                "the shallower one is pushed DOWN, never the other lifted up",
            );

            // The payoff of injecting the shift as margin BEFORE layout: the
            // row's own height is taffy's answer about the shifted children,
            // so nothing overflows a box that was sized without them.
            let container = scene.rect();
            assert!(
                small.y + small.h <= container.h,
                "the row contains the shifted item: {} + {} vs {}",
                small.y,
                small.h,
                container.h,
            );
            assert!(
                big.y + big.h <= container.h,
                "and still contains the unshifted one",
            );
        }

        fn box_px(w: u32, h: u32, margin_bottom: u32) -> Scene {
            Scene::Box(
                BoxNode::new(Rect::default(), BoxStyle::default()).with_layout(
                    LayoutStyle::new()
                        .with_size(Size::px(w, h))
                        .with_margin(Rect::new(0, 0, 0, margin_bottom)),
                ),
            )
        }

        /// R1641.5 — a child with no baseline of its own gets one SYNTHESIZED
        /// from its bottom margin edge, which is what CSS does.
        ///
        /// R1641 shipped this case top-aligned and documented the absence; the
        /// test that recorded it failed the moment the second pass landed,
        /// which is the right way for a documented boundary to move.
        #[test]
        #[allow(
            clippy::cast_precision_loss,
            reason = "a laid-out edge within a 200px viewport is exact in f32"
        )]
        fn r1641_5_a_box_is_aligned_by_its_bottom_margin_edge() {
            let mut c = cache();
            let mut scene = row(
                AlignItems::Baseline,
                vec![text("92.5", BIG), box_px(30, 30, 0)],
            );
            compute_layout(&mut scene, &mut c, 400, 200);

            let items = children_of(&scene);
            let (numeral, boxed) = (&items[0], items[1].rect());
            let text_baseline = absolute_baseline(numeral, &mut c);

            assert!(
                boxed.y > 0,
                "the box moved down to meet the text's baseline, where R1641 \
                 left it at the top",
            );
            assert!(
                ((boxed.y + boxed.h) as f32 - text_baseline).abs() <= 1.0,
                "its BOTTOM sits on the text's baseline: bottom={} baseline={}",
                boxed.y + boxed.h,
                text_baseline,
            );
        }

        /// The bottom MARGIN edge, not the border box: a declared bottom
        /// margin pushes the synthesized baseline further down, so the box
        /// rides higher.
        #[test]
        fn r1641_5_a_bottom_margin_is_part_of_the_synthesized_baseline() {
            let mut c = cache();
            let mut flush = row(
                AlignItems::Baseline,
                vec![text("92.5", BIG), box_px(30, 30, 0)],
            );
            let mut margined = row(
                AlignItems::Baseline,
                vec![text("92.5", BIG), box_px(30, 30, 8)],
            );
            compute_layout(&mut flush, &mut c, 400, 200);
            compute_layout(&mut margined, &mut c, 400, 200);

            let a = children_of(&flush)[1].rect().y;
            let b = children_of(&margined)[1].rect().y;
            assert_eq!(
                a.saturating_sub(b),
                8,
                "8px of bottom margin lifts the box by exactly 8px: {a} -> {b}",
            );
        }

        /// Two boxes and no text at all: they still align, because a
        /// synthesized baseline is a baseline.
        #[test]
        fn r1641_5_two_boxes_align_without_any_text() {
            let mut c = cache();
            let mut scene = row(
                AlignItems::Baseline,
                vec![box_px(20, 40, 0), box_px(20, 12, 0)],
            );
            compute_layout(&mut scene, &mut c, 400, 200);
            let items = children_of(&scene);
            let (tall, short) = (items[0].rect(), items[1].rect());
            assert_eq!(
                tall.y + tall.h,
                short.y + short.h,
                "both bottom edges land on the one baseline",
            );
            assert_eq!(tall.y, 0, "and the deepest one does not move");
        }

        #[test]
        fn r1641_a_single_child_is_a_no_op() {
            // Fewer than two boxes cannot be aligned TO each other. CSS
            // reaches the same place, and this is the only case that is still
            // a no-op now that every child can produce a baseline.
            let mut c = cache();
            let mut scene = row(AlignItems::Baseline, vec![box_px(30, 30, 0)]);
            compute_layout(&mut scene, &mut c, 400, 200);
            assert_eq!(children_of(&scene)[0].rect().y, 0);
        }

        #[test]
        fn r1641_a_column_is_untouched() {
            // Baseline is a CROSS-axis rule; a column's cross axis is
            // horizontal, where it has no meaning.
            let mut c = cache();
            let mut scene = Scene::Container(
                ContainerNode::new(vec![text("92.5", BIG), text("kg", SMALL)]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Column)
                        .with_align_items(AlignItems::Baseline),
                ),
            );
            compute_layout(&mut scene, &mut c, 400, 200);
            let items = children_of(&scene);
            assert_eq!(items[0].rect().y, 0, "first child at the top");
            assert_eq!(
                items[1].rect().y,
                items[0].rect().h,
                "second stacks directly below, with no baseline shift injected",
            );
        }

        #[test]
        fn r1641_a_declared_margin_survives_the_shift() {
            // The injected offset is ADDED to the application's own margin,
            // not substituted for it.
            let mut c = cache();
            let unmargined = {
                let mut s = row(
                    AlignItems::Baseline,
                    vec![text("92.5", BIG), text("kg", SMALL)],
                );
                compute_layout(&mut s, &mut c, 400, 200);
                children_of(&s)[1].rect().y
            };
            let mut margined = row(
                AlignItems::Baseline,
                vec![
                    text("92.5", BIG),
                    Scene::Text(
                        TextNode::styled(
                            "kg",
                            Rect::default(),
                            TextStyle::new().with_size_px(SMALL),
                        )
                        .with_layout(LayoutStyle::new().with_margin(Rect::new(0, 7, 0, 0))),
                    ),
                ],
            );
            compute_layout(&mut margined, &mut c, 400, 200);
            assert_eq!(
                children_of(&margined)[1].rect().y,
                unmargined + 7,
                "the declared 7px top margin is still 7px of the application's",
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // R55.D.6 §5.45 §5.21 — `LayoutStyle::absolute_position` substrate
    // (closes R55.D.4 spacer-flex workaround). The CSS-mirror contract:
    //
    //   1. A child with `absolute_position(left, top)` lands at
    //      `(parent.x + left, parent.y + top)` with its declared size.
    //   2. The child is removed from its parent's flex / block flow —
    //      siblings flow as if the absolute child were absent.
    //   3. The substrate's `Position::Relative` default (= None on
    //      `absolute_position`) leaves the existing layout graph
    //      bit-identical for every pre-R55.D.6 caller.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_d6_absolute_position_lands_child_at_offset() {
        // R55.D.6 — Container with one absolute-positioned child.
        // The child's declared `size` becomes the box dimensions; the
        // `absolute_position(40, 80)` builder lands it at (40, 80)
        // within the parent's content rect, ignoring flex flow.
        let abs_child = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default()).with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(20, 30))
                    .with_absolute_position(40, 80),
            ),
        );
        let mut scene = Scene::Container(
            ContainerNode::new(vec![abs_child])
                .with_layout(LayoutStyle::new().with_size(Size::px(200, 200))),
        );
        compute_layout(&mut scene, &mut cache(), 320, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(b) = &c.children[0] else {
            panic!("box child")
        };
        assert_eq!(b.rect.x, 40, "absolute left");
        assert_eq!(b.rect.y, 80, "absolute top");
        assert_eq!(b.rect.w, 20, "declared width");
        assert_eq!(b.rect.h, 30, "declared height");
    }

    #[test]
    fn r55_d6_absolute_child_removed_from_flex_flow() {
        // R55.D.6 — Flex Column with three children: two normal-flow
        // boxes and one absolute. The two normal-flow boxes stack
        // vertically as if the absolute child were absent (the third
        // box lands at y = 50 + gap=0 = 50, not y = 60 it would land
        // at if all three children were in flow).
        let in_flow = |h: u32| {
            Scene::Box(
                BoxNode::filled(Rect::default(), Color::default())
                    .with_layout(LayoutStyle::new().with_size(Size::px(100, h))),
            )
        };
        let abs_child = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default()).with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(10, 10))
                    .with_absolute_position(0, 0),
            ),
        );
        let mut scene = Scene::Container(
            ContainerNode::new(vec![in_flow(50), abs_child, in_flow(20)])
                .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
        );
        compute_layout(&mut scene, &mut cache(), 200, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(first) = &c.children[0] else {
            panic!("first")
        };
        let Scene::Box(third) = &c.children[2] else {
            panic!("third")
        };
        assert_eq!(first.rect.y, 0, "first in-flow stays at top");
        assert_eq!(
            third.rect.y, 50,
            "third in-flow stacks below first as if absolute child were absent"
        );
    }

    #[test]
    fn r55_d6_absolute_default_none_keeps_legacy_layout() {
        // R55.D.6 — the field defaults to None; a normal Flex Column
        // lays out identically to the pre-R55.D.6 substrate. This
        // pins backward compatibility so the entire example catalogue
        // stays bit-identical to the pre-R55.D.6 layout output.
        let leaf = |h: u32| {
            Scene::Box(
                BoxNode::filled(Rect::default(), Color::default())
                    .with_layout(LayoutStyle::new().with_size(Size::px(100, h))),
            )
        };
        let mut scene = Scene::Container(
            ContainerNode::new(vec![leaf(40), leaf(60)])
                .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(5)),
        );
        compute_layout(&mut scene, &mut cache(), 200, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(a) = &c.children[0] else {
            panic!("a")
        };
        let Scene::Box(b) = &c.children[1] else {
            panic!("b")
        };
        assert_eq!(a.rect.y, 0);
        assert_eq!(b.rect.y, 45, "gap=5 honored, no absolute interference");
    }

    // ─────────────────────────────────────────────────────────────────
    // R684 §5.21 — `LayoutStyle::flex_basis` taffy lowering. The
    // substrate-side `Option<SizeValue>` field maps to `taffy::Style::
    // flex_basis: Dimension`, and the four contract points below pin
    // the canonical translation rules:
    //
    //   1. `flex_basis = None` → `Dimension::Auto` (legacy intrinsic
    //      basis; bit-identical to pre-R684 layout).
    //   2. `flex_basis = Some(Px(0))` + `flex_grow > 0` distributes
    //      the FULL parent extent proportionally (the dock + splitter
    //      cascade R684 lands).
    //   3. `flex_basis = Some(Percent(p))` resolves to `p%` of the
    //      parent's main-axis size.
    //   4. Two siblings with `Px(0)` basis + different `flex_grow`
    //      values split the parent proportionally to the ratios.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r684_flex_basis_none_default_preserves_intrinsic_layout() {
        // Pre-R684 behaviour pin. A flex Row child with an explicit
        // `size: Size::px(120, 40)` and `flex_basis = None` must lay
        // out at exactly 120 px — the intrinsic basis matches the
        // declared size, the pre-R684 graph is unchanged.
        let layout = LayoutStyle::new().flex(FlexDirection::Row);
        let child = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default())
                .with_layout(LayoutStyle::new().with_size(Size::px(120, 40))),
        );
        let mut scene = Scene::Container(ContainerNode::new(vec![child]).with_layout(layout));
        compute_layout(&mut scene, &mut cache(), 360, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(b) = &c.children[0] else {
            panic!("box child")
        };
        assert_eq!(b.rect.w, 120, "None basis → intrinsic 120 px width");
    }

    #[test]
    fn r684_flex_basis_zero_px_with_flex_grow_one_takes_full_parent_extent() {
        // R684 contract atomic 1+2 anchor — the canonical
        // proportional-distribution idiom. A single flex Row child
        // with `flex_basis(Px(0))` + `flex_grow(1.0)` claims the
        // entire 360 px parent width, not just the leftover after
        // intrinsic content sizing. No explicit `with_size`: the
        // width-only assertion does not need a height pin (taffy
        // resolves height to 0 under the default cross-axis Stretch
        // when no `with_size` is supplied, which is what we want).
        let layout = LayoutStyle::new().flex(FlexDirection::Row);
        let child = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default()).with_layout(
                LayoutStyle::new()
                    .with_flex_basis(SizeValue::Px(0))
                    .with_flex_grow(1.0),
            ),
        );
        let mut scene = Scene::Container(ContainerNode::new(vec![child]).with_layout(layout));
        compute_layout(&mut scene, &mut cache(), 360, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(b) = &c.children[0] else {
            panic!("box child")
        };
        assert_eq!(
            b.rect.w, 360,
            "flex_basis(Px(0)) + flex_grow(1.0) must take the full parent extent",
        );
    }

    #[test]
    fn r684_flex_basis_percent_50_lays_out_at_half_parent() {
        // `Percent(50)` maps to `Dimension::percent(0.5)` — taffy
        // resolves the basis to half the parent's main-axis size on
        // the basis-resolution pass. With `flex_grow = 0` (default)
        // the layout result equals the basis: 180 px in a 360 px
        // parent.
        let layout = LayoutStyle::new().flex(FlexDirection::Row);
        let child = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default())
                .with_layout(LayoutStyle::new().with_flex_basis(SizeValue::Percent(50))),
        );
        let mut scene = Scene::Container(ContainerNode::new(vec![child]).with_layout(layout));
        compute_layout(&mut scene, &mut cache(), 360, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(b) = &c.children[0] else {
            panic!("box child")
        };
        assert_eq!(b.rect.w, 180, "Percent(50) basis → half-parent width");
    }

    #[test]
    fn r684_flex_basis_zero_with_flex_grow_ratios_split_proportionally() {
        // R684 atomic 2 anchor — the splitter ratio fix. Two siblings
        // with `flex_basis(Px(0))` + ratios 0.3 and 0.7 must split
        // the 1000-wide parent into 300 + 700 (proportional), NOT
        // ~500 + ~500 (the pre-R684 `Auto` basis collapsed every
        // ratio to roughly 50/50 because the leftover-distribution
        // path operates on a near-zero remainder when both children
        // already claim full intrinsic width).
        let parent_w: u32 = 1000;
        let left = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default()).with_layout(
                LayoutStyle::new()
                    .with_flex_basis(SizeValue::Px(0))
                    .with_flex_grow(0.3),
            ),
        );
        let right = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default()).with_layout(
                LayoutStyle::new()
                    .with_flex_basis(SizeValue::Px(0))
                    .with_flex_grow(0.7),
            ),
        );
        let layout = LayoutStyle::new().flex(FlexDirection::Row);
        let mut scene = Scene::Container(ContainerNode::new(vec![left, right]).with_layout(layout));
        compute_layout(&mut scene, &mut cache(), parent_w, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(l) = &c.children[0] else {
            panic!("left")
        };
        let Scene::Box(r) = &c.children[1] else {
            panic!("right")
        };
        // Allow a 1-px tolerance for rounding (taffy rounds f32
        // pixels to u32 at the layout-apply boundary).
        assert!(
            l.rect.w.abs_diff(300) <= 1,
            "0.3 ratio child must take 300 px ± 1 of {parent_w} px parent, got {}",
            l.rect.w,
        );
        assert!(
            r.rect.w.abs_diff(700) <= 1,
            "0.7 ratio child must take 700 px ± 1 of {parent_w} px parent, got {}",
            r.rect.w,
        );
        // The total must equal the parent's main-axis extent (the
        // children fully cover the row, no leftover gap).
        assert_eq!(l.rect.w + r.rect.w, parent_w);
    }

    #[test]
    fn r684_flex_basis_some_auto_equivalent_to_none() {
        // The `to_dimension(SizeValue::Auto)` fallback feeds
        // `taffy::Dimension::Auto`, identical to the `None` branch.
        // Two scenes constructed with `None` vs `Some(Auto)` must
        // lay out bit-identical so callers can reset a previously-
        // set basis via `with_flex_basis(Auto)` without rebuilding
        // the entire LayoutStyle.
        let layout = LayoutStyle::new().flex(FlexDirection::Row);
        let intrinsic_child = |basis: Option<SizeValue>| -> Scene {
            let mut ls = LayoutStyle::new().with_size(Size::px(140, 40));
            if let Some(b) = basis {
                ls = ls.with_flex_basis(b);
            }
            Scene::Box(BoxNode::filled(Rect::default(), Color::default()).with_layout(ls))
        };

        let mut none_scene = Scene::Container(
            ContainerNode::new(vec![intrinsic_child(None)]).with_layout(layout.clone()),
        );
        let mut some_auto_scene = Scene::Container(
            ContainerNode::new(vec![intrinsic_child(Some(SizeValue::Auto))]).with_layout(layout),
        );
        compute_layout(&mut none_scene, &mut cache(), 360, 200);
        compute_layout(&mut some_auto_scene, &mut cache(), 360, 200);

        let extract_w = |s: &Scene| -> u32 {
            let Scene::Container(c) = s else {
                panic!("container")
            };
            let Scene::Box(b) = &c.children[0] else {
                panic!("box")
            };
            b.rect.w
        };
        assert_eq!(extract_w(&none_scene), extract_w(&some_auto_scene));
        assert_eq!(extract_w(&none_scene), 140, "intrinsic 140 preserved");
    }

    /// R1070 §5.37 — forcing consumer for the measure arm. With the self-hosted
    /// engine supplied, an eligible single-style `Scene::Text` leaf is sized by
    /// the §5.37 engine, NOT by parley: the measured box width equals the §5.37
    /// shaped advance (ceil-snapped). The §5.37 paint arm declines whenever
    /// `advance > rect.w`, so a box this wide guarantees the paint arm renders the
    /// single line *inside* the box — zero overflow. This closes the R1068
    /// paint-only gap where measure stayed parley and a string wider than the
    /// §5.37 advance could overflow the parley box. `compute_layout` (no override)
    /// is unchanged.
    #[cfg(feature = "vello")]
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "advance (asserted > 0) / rect.w are small px values — exact in the f32/u32 round-trip"
    )]
    fn self_hosted_measure_box_registers_with_paint_advance() {
        use crate::text_engine::SelfHostedTextEngine;
        use pinion_text_font::{Font, shape_paragraph_with_fallback};

        const NOTO: &[u8] =
            include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");
        let font = Font::from_bytes(NOTO.to_vec()).expect("parse NotoSans fixture");
        let px = 18.0_f32;
        // The exact §5.37 advance the paint arm shapes; its decline test is
        // `advance > rect.w`, so the measured box must be at least this wide.
        let advance = shape_paragraph_with_fallback(&[&font], "Measure", px).advance;
        let expected_w = advance.ceil() as u32;
        assert!(
            advance > 0.0,
            "ink-bearing text shapes to a positive advance"
        );

        let engine = SelfHostedTextEngine::from_font(font);
        let make_scene = || {
            // Start-aligned flex row in a viewport far wider than the text, so the
            // leaf measures at its intrinsic single-line size (no wrap pressure).
            // `align_items: Start` keeps the cross axis (height) intrinsic too — the
            // default `Stretch` would inflate rect.h to the container, masking the
            // §5.37 box height.
            let text = Scene::Text(TextNode::styled(
                "Measure",
                Rect::default(),
                TextStyle::new().with_size_px(18),
            ));
            Scene::Container(
                ContainerNode::new(vec![text]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Start),
                ),
            )
        };
        let measured = |scene: &Scene| -> (u32, u32) {
            let Scene::Container(c) = scene else {
                panic!("container")
            };
            let Scene::Text(t) = &c.children[0] else {
                panic!("text")
            };
            (t.rect.w, t.rect.h)
        };
        // Independent (non-LineBoxMetrics) expected height = the ceil'd hhea line box.
        // For NotoSans typo == hhea, so this raw-hhea oracle equals the R1079
        // USE_TYPO_METRICS-selected box `LineBoxMetrics::from_font` now uses; a
        // typo != hhea font would need `vertical_line_metrics` here (see the §5.37
        // `box_parity_matches_parley_metric_engine_via_skrifa` NanumGothic case).
        let f = engine.font();
        let upem = f64::from(f.units_per_em());
        let expected_h = ((f64::from(f.ascender()) - f64::from(f.descender())
            + f64::from(f.line_gap()))
            * f64::from(px)
            / upem)
            .ceil() as u32;

        // §5.37 measure: the box (w, h) equals the §5.37 advance.ceil × hhea line box,
        // proving the override took effect (the leaf is sized by §5.37, not parley).
        // Width == advance.ceil holds HERE because the leaf is laid out at its
        // INTRINSIC size — a Start flex row with no grow/shrink in a viewport far
        // wider than the text. Under flex grow/shrink taffy would resolve a different
        // rect.w; the universal invariant the paint arm relies on is the weaker
        // `advance <= rect.w` (no overflow), asserted next.
        let mut some_scene = make_scene();
        let mut cache_some = LayoutCache::new();
        let _ = compute_layout_with_text_measure(
            &mut some_scene,
            &mut cache_some,
            800,
            200,
            Some(&engine),
        );
        let (some_w, some_h) = measured(&some_scene);
        assert_eq!(
            some_w, expected_w,
            "the intrinsic §5.37 box width must equal the §5.37 shaped advance (ceil)"
        );
        assert_eq!(
            some_h, expected_h,
            "the §5.37 box height must equal the hhea line box (ceil)"
        );
        // Coherence (the UNIVERSAL invariant): the paint arm's decline test
        // `advance > rect.w` is false, so it paints the single §5.37 line inside the
        // box — zero overflow. This holds under grow/shrink too (a grown box is only
        // wider), unlike the exact-width equality above.
        assert!(
            advance <= some_w as f32,
            "the §5.37 paint advance ({advance}) must fit the §5.37 measured box ({some_w})"
        );

        // Regression: with no override the leaf measures through parley exactly as
        // before (`compute_layout` threads `None`) — a non-zero parley box.
        let mut none_scene = make_scene();
        let mut cache_none = LayoutCache::new();
        compute_layout(&mut none_scene, &mut cache_none, 800, 200);
        assert!(
            measured(&none_scene).0 > 0,
            "the parley measure path is unchanged and still produces a non-zero box"
        );
    }

    /// R1072 §5.37 — the caret-bearing marker flows `TextNode` →
    /// `NodeContext::Text` → the measure callback → the shared eligibility SSOT,
    /// so even with an engine override a caret-bearing (editable) leaf measures
    /// through parley — keeping its parley-shaped caret / selection aligned. Its
    /// box is the parley box (engine on == engine off), not the §5.37 advance.
    #[cfg(feature = "vello")]
    #[test]
    fn caret_bearing_leaf_defers_to_parley_measure_through_node_context() {
        use crate::text_engine::SelfHostedTextEngine;
        use pinion_text_font::Font;

        const NOTO: &[u8] =
            include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");
        let font = Font::from_bytes(NOTO.to_vec()).expect("parse NotoSans fixture");
        let engine = SelfHostedTextEngine::from_font(font);

        let make_scene = || {
            let text = Scene::Text(
                TextNode::styled(
                    "Measure",
                    Rect::default(),
                    TextStyle::new().with_size_px(18),
                )
                .caret_bearing(),
            );
            Scene::Container(
                ContainerNode::new(vec![text]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Start),
                ),
            )
        };
        let measured = |scene: &Scene| -> (u32, u32) {
            let Scene::Container(c) = scene else {
                panic!("container")
            };
            let Scene::Text(t) = &c.children[0] else {
                panic!("text")
            };
            (t.rect.w, t.rect.h)
        };

        // Engine supplied, but the leaf is caret-bearing → deferred to parley.
        let mut on_scene = make_scene();
        let mut cache_on = LayoutCache::new();
        let _ =
            compute_layout_with_text_measure(&mut on_scene, &mut cache_on, 800, 200, Some(&engine));

        // Parley baseline (no override).
        let mut off_scene = make_scene();
        let mut cache_off = LayoutCache::new();
        compute_layout(&mut off_scene, &mut cache_off, 800, 200);

        assert_eq!(
            measured(&on_scene),
            measured(&off_scene),
            "a caret-bearing leaf measures identically with the engine on or off \
             (excluded from §5.37 — both arms together)"
        );
    }

    /// R1479 §5.37 — the leaf's FAMILY reaches the eligibility decision through
    /// the same `TextNode` → `NodeContext::Text` → measure-callback path as the
    /// caret marker above, so a leaf naming a family the engine does not hold is
    /// sized by parley — the shaper that can actually select that face.
    ///
    /// Asserted as engine-on == engine-off rather than against a number: what
    /// parley resolves the name to is a property of the host, but that the
    /// override changes nothing is a property of this code, and it is the one
    /// under test. The positive control (a leaf naming nothing IS sized by
    /// §5.37) is what keeps this from passing by disabling the arm.
    #[cfg(feature = "vello")]
    #[test]
    fn r1479_leaf_naming_a_foreign_family_defers_to_parley_measure() {
        use crate::text_engine::SelfHostedTextEngine;
        use pinion_text_font::Font;

        const NOTO: &[u8] =
            include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");
        const NANUM: &[u8] =
            include_bytes!("../../pinion-text-font/tests/fonts/NanumGothic-Regular.ttf");

        let engine = SelfHostedTextEngine::from_font(
            Font::from_bytes(NOTO.to_vec()).expect("parse NotoSans fixture"),
        );
        let served = engine
            .served_family()
            .expect("the NotoSans fixture declares a family");
        let foreign = Font::from_bytes(NANUM.to_vec())
            .expect("parse NanumGothic fixture")
            .family_name()
            .expect("the NanumGothic fixture declares a family");
        assert_ne!(
            served, foreign,
            "premise: the fixtures name different faces"
        );

        let make_scene = |style: TextStyle| {
            move || {
                let text = Scene::Text(TextNode::styled("Measure", Rect::default(), style.clone()));
                Scene::Container(
                    ContainerNode::new(vec![text]).with_layout(
                        LayoutStyle::new()
                            .flex(FlexDirection::Row)
                            .with_align_items(AlignItems::Start),
                    ),
                )
            }
        };
        let measured = |scene: &Scene| -> (u32, u32) {
            let Scene::Container(c) = scene else {
                panic!("container")
            };
            let Scene::Text(t) = &c.children[0] else {
                panic!("text")
            };
            (t.rect.w, t.rect.h)
        };
        let lay = |scene: &mut Scene, engine: Option<&SelfHostedTextEngine>| {
            let mut cache = LayoutCache::new();
            let m = engine.map(|e| e as &dyn TextMeasure);
            let _ = compute_layout_with_text_measure(scene, &mut cache, 800, 200, m);
        };

        // A leaf that names the OTHER fixture's family.
        let named = make_scene(TextStyle::new().with_size_px(18).with_font_family(foreign));
        let (mut on, mut off) = (named(), named());
        lay(&mut on, Some(&engine));
        lay(&mut off, None);
        assert_eq!(
            measured(&on),
            measured(&off),
            "a leaf naming a family the engine does not hold measures through \
             parley, engine on or off",
        );

        // Positive control: with the family the engine DOES hold, the override
        // takes effect — so the equality above is a decline and not an inert
        // arm. Asserted against the §5.37 advance computed from the fixture's
        // own bytes, NOT against parley: what parley resolves a name to is the
        // host's business, and a control that asked it would pass or fail by
        // which fonts the runner has installed.
        let own = make_scene(
            TextStyle::new()
                .with_size_px(18)
                .with_font_family(served.clone()),
        );
        let mut own_on = own();
        lay(&mut own_on, Some(&engine));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a single line advance is a small positive px value"
        )]
        let s537_w =
            pinion_text_font::shape_paragraph_with_fallback(&[engine.font()], "Measure", 18.0_f32)
                .advance
                .ceil() as u32;
        assert_eq!(
            measured(&own_on).0,
            s537_w,
            "a leaf naming {served} IS sized by the §5.37 arm (intrinsic width \
             == the §5.37 advance), so the override reached this layout pass",
        );
    }

    /// R1447 §5.36 §5.37 — the `TextMeasure` seam decides whether a layout
    /// pass touches fonts at all, and [`LayoutCache`] now defers the system
    /// font scan to the first shape, so "touches fonts" is observable.
    ///
    /// Both arms run the same scene through the same entry point; only the
    /// measure impl differs. An impl that answers **every** leaf (what
    /// `pinion_tui::text_layout::CellTextLayout` is — it lays text out on the
    /// cell grid) leaves the shaper unreached, so the cache never builds a
    /// `FontContext`. `None` — the GUI/parley arm — builds one.
    ///
    /// This is the generic statement about the seam; `pinion-tui` holds the
    /// TUI-specific half, and deliberately holds only the half that needs no
    /// installed font.
    #[test]
    fn r1447_answering_measure_arm_leaves_the_font_scan_unrun() {
        /// Answers every leaf with a fixed box — the "never defers" shape.
        struct AlwaysMeasures;
        impl TextMeasure for AlwaysMeasures {
            fn measure_text(
                &self,
                content: &str,
                _style: &TextStyle,
                _runs: &[StyleRun],
                _max_width: Option<u32>,
                _caret_bearing: bool,
            ) -> Option<TextBox> {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "test fixture content is a handful of chars"
                )]
                Some(TextBox::single_line(
                    content.chars().count() as f32 * 8.0,
                    16.0,
                ))
            }
        }

        fn scene() -> Scene {
            let mut text = TextNode::default();
            text.content = "text that would have to be shaped".to_owned();
            Scene::Container(ContainerNode::new(vec![Scene::Text(text)]))
        }

        let mut answered = cache();
        let _ = compute_layout_with_text_measure(
            &mut scene(),
            &mut answered,
            320,
            200,
            Some(&AlwaysMeasures),
        );
        assert_eq!(
            answered.font_scans(),
            0,
            "a measure arm that answers every leaf never reaches the shaper, \
             so the system font scan never runs",
        );

        let mut deferred = cache();
        let _ = compute_layout_with_text_measure(&mut scene(), &mut deferred, 320, 200, None);
        assert_eq!(
            deferred.font_scans(),
            1,
            "premise: this scene's text does reach parley with no measure arm \
             — otherwise the assertion above is about a scene with no text",
        );
    }

    /// R1465 §5.45 — the lifted fixed point's own invariants, the ones five
    /// hand-written copies each had to get right.
    mod settle {
        use super::super::{SETTLE_PASS_BUDGET, settle_to_fixed_point};
        use pinion_core::Scene;
        use pinion_core::scene::{BoxNode, Rect};
        use pinion_core::style::BoxStyle;
        use std::cell::Cell;

        /// R1554 — these three tests are about the LOOP's arithmetic, not about
        /// any scene's content, so they carry the settling bound in the one
        /// field of a one-node scene. They used a bare `u32` as the scene type
        /// until `settle_to_fixed_point` became concrete over [`Scene`] (which
        /// is what lets the disabled cascade ride the loop); the carrier
        /// changed, the invariants did not.
        fn scalar(n: u32) -> Scene {
            Scene::Box(BoxNode::new(Rect::new(n, 0, 1, 1), BoxStyle::default()))
        }

        fn value(scene: &Scene) -> u32 {
            match scene {
                Scene::Box(b) => b.rect.x,
                other => panic!("not the scalar carrier: {other:?}"),
            }
        }

        fn bump(scene: &mut Scene) -> u32 {
            match scene {
                Scene::Box(b) => {
                    b.rect.x += 1;
                    b.rect.x
                }
                other => panic!("not the scalar carrier: {other:?}"),
            }
        }

        #[test]
        fn a_converging_chain_stops_at_its_fixed_point() {
            // The bound the layout pass writes and the view reads back is the
            // same shape a scroll bound has: pass N's output is pass N+1's
            // input. A fixture whose view ignored it would measure nothing.
            let bound = Cell::new(0_u32);
            let settle = settle_to_fixed_point(
                || scalar(bound.get()),
                |scene: &mut Scene| {
                    if value(scene) < 3 {
                        bound.set(bump(scene));
                        true
                    } else {
                        false
                    }
                },
            );
            assert!(settle.settled, "it arrived");
            assert_eq!(settle.passes, 4, "three that grew it, one that agreed");
            assert_eq!(
                value(&settle.scene),
                3,
                "and the scene is the fixed point's"
            );
        }

        #[test]
        fn a_chain_that_never_agrees_is_bounded_and_says_so() {
            let settle = settle_to_fixed_point(
                || scalar(0),
                |scene: &mut Scene| {
                    bump(scene);
                    true
                },
            );
            assert_eq!(settle.passes, SETTLE_PASS_BUDGET, "bounded, not hung");
            assert!(
                !settle.settled,
                "and it reports that it gave up — a caller that read `passes` \
                 alone could not tell this from converging ON the budget",
            );
        }

        #[test]
        fn every_view_run_is_laid_out() {
            // The invariant a copy gets wrong silently: re-running the view on
            // the LAST pass would return a scene with no layout behind it,
            // every rect at its default. Equal counts is what forbids it.
            for (label, mut step) in [
                (
                    "converging",
                    Box::new(|s: &mut Scene| bump(s) < 3) as Box<dyn FnMut(&mut Scene) -> bool>,
                ),
                ("runaway", Box::new(|_: &mut Scene| true)),
            ] {
                let views = Cell::new(0_u32);
                let settle = settle_to_fixed_point(
                    || {
                        views.set(views.get() + 1);
                        scalar(0)
                    },
                    |scene: &mut Scene| step(scene),
                );
                assert_eq!(
                    views.get(),
                    settle.passes,
                    "{label}: the loop never runs a view it does not lay out",
                );
            }
        }
    }
}

// ── R1607 §5.21 — does the R1560 grid hold a TILE DASHBOARD? ──

#[cfg(test)]
mod tile_dashboard_measurement {
    use super::*;
    use pinion_core::style::{Display, GridPlacement, GridTrack, LayoutStyle};

    fn cache() -> LayoutCache {
        LayoutCache::new()
    }

    /// A dashboard of the class a monitoring tool draws is a fixed set of
    /// equal columns with cards placed at `(col, row)` covering `(w, h)` of
    /// them. R1560 added CSS Grid for a
    /// text table's rowspans; the tile-dashboard debt predicted that the SAME
    /// layout holds a dashboard and asked for the prediction to be **measured
    /// before a second layout kind is built**. This is that measurement, and
    /// it is a test rather than a paragraph.
    #[test]
    fn r1607_twelve_equal_columns_place_spanning_cards_where_a_dashboard_wants_them() {
        let tile = |col: u16, row: u16, w: u16, h: u16| {
            Scene::Container(
                ContainerNode::new(Vec::new()).with_layout(
                    LayoutStyle::default()
                        .with_grid_column(GridPlacement::spanning(col, w))
                        .with_grid_row(GridPlacement::spanning(row, h)),
                ),
            )
        };
        // The classic layout: a full-width header, then two halves, then a
        // third that is twice as tall as the row above it.
        let mut scene = Scene::Container(
            ContainerNode::new(vec![
                tile(1, 1, 12, 1),
                tile(1, 2, 6, 1),
                tile(7, 2, 6, 1),
                tile(1, 3, 4, 2),
            ])
            .with_layout(
                LayoutStyle::default()
                    .grid_columns(vec![GridTrack::Fr(1.0); 12])
                    .with_grid_rows(vec![GridTrack::Px(50); 4]),
            ),
        );
        assert_eq!(
            scene.layout_style().map(|l| l.display),
            Some(Display::Grid),
            "declaring tracks is what puts a container in grid mode"
        );

        compute_layout(&mut scene, &mut cache(), 1200, 400);
        let Scene::Container(root) = &scene else {
            panic!("a container")
        };
        let rect = |i: usize| root.children[i].rect();

        // 1200 / 12 = 100 per column, 50 per row.
        assert_eq!((rect(0).w, rect(0).h), (1200, 50), "the full-width header");
        assert_eq!((rect(1).x, rect(1).w), (0, 600), "the left half");
        assert_eq!((rect(2).x, rect(2).w), (600, 600), "the right half");
        assert_eq!(rect(1).y, 50, "and it sits on the second row");
        assert_eq!(
            (rect(3).x, rect(3).y, rect(3).w, rect(3).h),
            (0, 100, 400, 100),
            "★ a card two rows tall — the thing a nest of flex rows cannot express, \
             which is the whole reason a dashboard needed a grid and not a splitter tree"
        );
    }

    /// The other half of the prediction: a splitter tree could not do this.
    /// Widening one card must not re-shape its neighbours' tracks, because the
    /// tracks are sized once across the container rather than per row.
    #[test]
    fn r1607_widening_one_card_leaves_the_other_rows_columns_alone() {
        let tile = |col: u16, row: u16, w: u16| {
            Scene::Container(
                ContainerNode::new(Vec::new()).with_layout(
                    LayoutStyle::default()
                        .with_grid_column(GridPlacement::spanning(col, w))
                        .with_grid_row(GridPlacement::at(row)),
                ),
            )
        };
        let build = |first_width: u16| {
            Scene::Container(
                ContainerNode::new(vec![tile(1, 1, first_width), tile(1, 2, 3), tile(4, 2, 3)])
                    .with_layout(
                        LayoutStyle::default()
                            .grid_columns(vec![GridTrack::Fr(1.0); 6])
                            .with_grid_rows(vec![GridTrack::Px(40); 2]),
                    ),
            )
        };
        let mut narrow = build(2);
        let mut wide = build(5);
        compute_layout(&mut narrow, &mut cache(), 600, 200);
        compute_layout(&mut wide, &mut cache(), 600, 200);

        let row_two = |scene: &Scene| {
            let Scene::Container(root) = scene else {
                panic!("a container")
            };
            (root.children[1].rect().w, root.children[2].rect().x)
        };
        assert_eq!(
            row_two(&narrow),
            row_two(&wide),
            "row two is untouched by row one's width — a splitter tree would have \
             had to re-divide the whole parent"
        );
        assert_eq!(row_two(&narrow), (300, 300));
    }
}
