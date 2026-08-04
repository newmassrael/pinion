//! R46.3.1 §5.16 `paint_adapter` — Scene → `vello::Scene` framework
//! primitive.
//!
//! Replaces the inline Scene-walker that lived in ai-introspect-demo
//! R46.3 (`build_vello_scene` / `fill_rect` / `stroke_rect` /
//! `pinion_to_peniko` / `root_background`). Promoted here so additional
//! consumers (hello-button R46.5+, future widget catalog) share one
//! Scene → Vello translation path instead of re-implementing it per
//! example — the same lesson R48 codified for input dispatch
//! (application-level workaround → framework primitive).
//!
//! Application-specific tag substitution is exposed as a closure hook
//! ([`to_vello`] generic over `Fn(&BoxNode) -> Option<Color>`) so the
//! framework module stays free of application tags (e.g. the demo's
//! `info_panel` palette indexing). Pass `&|_| None` when no override
//! is required.
//!
//! Border placement (R46.3.2) honours
//! [`pinion_core::style::BorderPlacement`]:
//!
//! * `Inside` (default, legacy softbuffer behaviour) — centred stroke
//!   inset by `width/2` so the whole stroke lies within `rect`.
//! * `Center` — Vello's native stroke (half-width spills outside).
//! * `Outside` — centred stroke offset by `width/2` outwards.
//!
//! R47.3 §5.36 — [`Scene::Text`] paints via parley-shaped glyph runs.
//! The caller passes a `&mut pinion_text::LayoutCache` so steady-state
//! frames hit the cache instead of re-shaping every label; the cache
//! also owns the parley `FontContext` / `LayoutContext` so the
//! framework module never holds parley state across calls.
//! `parley::FontData = peniko::FontData` (re-exported via
//! `linebender_resource_handle`), so the run's font feeds
//! [`vello::Scene::draw_glyphs`] unchanged.
//!
//! Available only under the `vello` feature; non-GUI consumers
//! (headless / TUI / future paint backends) compile without wgpu
//! transitively.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::draw_profile::DrawProfiler;
use crate::frame_timing::DrawWork;
use crate::image_cache::ImageCache;
use crate::paint_cache_stats::FragmentCacheStats;
use pinion_core::Scene;
use pinion_core::cell_metric::CellMetric;
use pinion_core::scene::{
    BoxNode, ImageNode, ImmediateModeNode, ImmediatePainter, PathCommand, PathNode, PathPoint,
    Rect, TextGridNode, TextNode,
};
use pinion_core::style::{
    Border, BorderPlacement, BoxStyle, Color, Fit, FontStyle, FontWeight, GenericFontFamily,
    Gradient, GradientKind, LineHeight, StrokeCap, TextOverflow, TextStyle,
};
use pinion_core::term_grid::{
    CellWidth, ColorTarget, CursorShape, GridBuffer, Palette, TermCell, TermColor, UnderlineStyle,
};
use pinion_text::LayoutCache;
use pinion_text::PositionedRun;
use vello::Glyph;
use vello::Scene as VelloScene;
use vello::kurbo::{
    Affine, BezPath, Cap as KurboCap, Line, PathEl, Point as KurboPoint, Rect as KurboRect,
    RoundedRect as KurboRoundedRect, Stroke,
};
use vello::peniko::{
    Blob, Brush as PenikoBrush, Color as PenikoColor, Extend as PenikoExtend, Fill,
    Gradient as PenikoGradient, ImageAlphaType, ImageBrush, ImageData, ImageFormat, ImageQuality,
};
// R1063 §5.37 → production-paint seam — the self-hosted text engine's CPU AA
// coverage mask, plus (R1065) the per-glyph [`GlyphAtlas`] surface a per-glyph
// paint path samples. `pinion-text-font` is gated to the `vello` feature (see
// the crate Cargo.toml), so these imports are reachable only inside the
// vello-only paint_adapter module.
use pinion_text_font::{
    Coverage, GlyphAtlas, RenderedGlyphs, render_paragraph_atlased, shape_paragraph_with_fallback,
};
// R1068 §5.37 → production Scene::Text — the opt-in self-hosted paint arm's
// cached font handle (the parley path stays the default; see
// [`to_vello_with_text_engine`]).
use crate::text_engine::{LineBoxMetrics, SelfHostedTextEngine, single_line_overflows};

/// Build a Vello scene from a pinion [`Scene`] tree. `fill_hook` is
/// consulted for each [`BoxNode`] visited; a `Some(color)` return
/// overrides the box's native `style.fill`, `None` keeps it. Pass
/// `&|_: &BoxNode| None` when no tag-based substitution is needed.
///
/// `text_cache` is the per-application [`LayoutCache`] (R47.3 §5.36) —
/// caching parley `Layout` values across frames so static labels do not
/// re-shape every redraw. Pass `&mut LayoutCache::new()` only when the
/// caller knows the scene contains zero `Scene::Text` (the cache is
/// otherwise dormant); long-lived applications should own one.
///
/// Walk semantics (R47.3 §5.36):
///
/// * [`Scene::Container`] — fill `rect` with `style.fill`, recurse
///   into `children`.
/// * [`Scene::Box`] — fill `rect` with `fill_hook(b)` or
///   `b.style.fill`; stroke `b.style.border` when present.
/// * [`Scene::Text`] — shape via [`LayoutCache::layout`], walk
///   `positioned_glyphs()` per `parley::GlyphRun`, emit one
///   [`vello::Scene::draw_glyphs`] call per run.
/// * [`Scene::Path`] — lower `commands` (rect-relative since R1358) to
///   a Vello [`BezPath`] and fill (`style.fill`, non-zero winding) +
///   stroke (`style.stroke`) via `paint_path` (R721).
/// * [`Scene::External`] / [`Scene::Effect`] / [`Scene::Image`] —
///   no-op. The Image paint primitive attaches in a follow-up round.
pub fn to_vello<F>(scene: &Scene, fill_hook: &F, text_cache: &mut LayoutCache, out: &mut VelloScene)
where
    F: Fn(&BoxNode) -> Option<Color>,
{
    // R1068 §5.37 — the default path supplies no self-hosted engine, so
    // `Scene::Text` paints via parley exactly as before (byte-identical).
    to_vello_with_text_engine(scene, fill_hook, text_cache, None, out);
}

/// R1068 §5.37 → production `Scene::Text` — [`to_vello`] with an opt-in
/// self-hosted text engine.
///
/// When `engine` is `Some`, **single-style, single-line, undecorated**
/// `Scene::Text` leaves paint through the §5.37 self-hosted engine
/// (`shape_paragraph_with_fallback` → `render_paragraph_atlased` →
/// [`draw_atlased_glyphs`]) instead of parley; every other text (styled runs,
/// wrapped / multi-line, underline / strikethrough) still takes the parley
/// path, and `engine = None` is byte-identical to the pre-R1068 [`to_vello`].
/// Layout / measure is unchanged (still parley), so caret hit-testing and text
/// editing are unaffected — this arm swaps paint only. The opt-in flag the
/// campaign describes is the caller's choice to pass `Some(&engine)`.
pub fn to_vello_with_text_engine<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    engine: Option<&SelfHostedTextEngine>,
    out: &mut VelloScene,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    // R740 §5.16 — the uncached walker is the stateless reference /
    // test path (production uses `to_vello_cached` with the per-window
    // persistent cache). A per-call throwaway `ImageCache` keeps this
    // public signature stable while still painting `Scene::Image`
    // leaves; there is no cross-call decode reuse here by design.
    let mut image_cache = ImageCache::new();
    to_vello_inner(
        scene,
        fill_hook,
        text_cache,
        &mut image_cache,
        engine,
        out,
        Affine::IDENTITY,
    );
}

/// (R51.188 §5.45 R55.E.1) Transform-carrying recursive walker.
///
/// The public [`to_vello`] forwards [`Affine::IDENTITY`]; the
/// internal recursion threads the cumulative transform through so
/// the [`Scene::Scroll`] arm can compose
/// `parent_transform * Affine::translate((viewport.xy - offset.xy))`
/// before recursing into the scroll content. Every leaf paint
/// primitive ([`fill_rect`] / [`stroke_rect`] / [`paint_text`])
/// applies the threaded transform at the Vello call site so the
/// content paints offset into the viewport without mutating the
/// scene tree.
///
/// `Scene::Scroll` pushes a Vello clip layer keyed to the viewport
/// rect (in the parent transform's frame), then recurses with the
/// shifted child transform. `pop_layer` balances on exit so the
/// adapter stays compatible with Vello's stack contract.
#[allow(clippy::too_many_arguments)]
fn to_vello_inner<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    image_cache: &mut ImageCache,
    engine: Option<&SelfHostedTextEngine>,
    out: &mut VelloScene,
    transform: Affine,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    match scene {
        Scene::Container(c) => {
            paint_box_decoration(out, c.rect, &c.style, c.style.fill, transform);
            for child in &c.children {
                to_vello_inner(
                    child,
                    fill_hook,
                    text_cache,
                    image_cache,
                    engine,
                    out,
                    transform,
                );
            }
        }
        Scene::Box(b) => {
            let fill = fill_hook(b).unwrap_or(b.style.fill);
            paint_box_decoration(out, b.rect, &b.style, fill, transform);
        }
        Scene::Text(t) => paint_text(out, t, text_cache, engine, transform),
        Scene::Scroll(s) => {
            // R55.E.1 — viewport clip in the parent's coordinate
            // frame; Vello applies the supplied transform to the
            // clip shape so the resulting screen-space clip lands
            // at `transform * viewport`.
            let viewport_clip = KurboRect::new(
                f64::from(s.viewport.x),
                f64::from(s.viewport.y),
                f64::from(s.viewport.x.saturating_add(s.viewport.w)),
                f64::from(s.viewport.y.saturating_add(s.viewport.h)),
            );
            out.push_clip_layer(Fill::NonZero, transform, &viewport_clip);
            // Content paints in content-intrinsic coordinates; the
            // scroll container shifts so that content-intrinsic
            // `(0, 0)` lands at viewport `(viewport.x - offset_x,
            //  viewport.y - offset_y)` in the parent frame. Compose
            // with the inherited transform so nested scrolls
            // accumulate correctly.
            let dx = f64::from(s.viewport.x) - f64::from(s.offset_x);
            let dy = f64::from(s.viewport.y) - f64::from(s.offset_y);
            let child_transform = transform * Affine::translate((dx, dy));
            to_vello_inner(
                &s.content,
                fill_hook,
                text_cache,
                image_cache,
                engine,
                out,
                child_transform,
            );
            out.pop_layer();
        }
        // R681 §2 #4 — ImmediateModeNode paints through the
        // backend-agnostic [`ImmediatePainter`] surface. The shell's
        // [`pinion_shell::ShellCore::compute_paint_scene_internal`]
        // has already advanced the driver via the R831 fixed-timestep
        // accumulator (`node.handle.borrow_mut().tick(FIXED)`) by the
        // time the paint walker reaches this branch (the tick + paint
        // phases are separated so the per-window
        // [`winit::event_loop::ControlFlow::WaitUntil`] game-loop pacing
        // drives the tick independently of the encode step). The painter
        // composes `transform * translate(viewport.{x,y})` so the
        // driver paints in viewport-LOCAL coordinates and the
        // result lands at the correct screen-space position.
        Scene::ImmediateModeNode(node) => {
            paint_immediate_mode_node(out, node, transform);
        }
        Scene::Path(p) => paint_path(out, p, transform),
        // R740 §5.16 — raster image paint (decode-once via `image_cache`).
        Scene::Image(i) => paint_image(out, i, image_cache, transform),
        // R991 §5.41 §2 #6 — cell-native terminal grid glyph paint (the
        // deferred half of the §5.41 axis: R972–R978 shipped the data
        // model, this arm rasterises it).
        // R1426 §5.41 — the uncached reference / test walker always paints the
        // cursor STEADY (`cursor_blink_on = true`): it is the stateless
        // reference path (production uses the cached walker), so it carries no
        // per-window blink clock and never animates the phase. R1427 — it is
        // likewise always FOCUSED (`cursor_focused = true`): OS focus is a live
        // per-window shell fact, so this stateless path draws the filled cursor.
        Scene::TextGrid(n) => {
            paint_text_grid(
                out,
                n,
                text_cache,
                transform,
                CursorPaintFlags::STEADY_FOCUSED,
            );
        }
        // External / Effect: no-op (no rasterizable contribution here).
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// R682 §5.16 atomic 1 — paint-fragment cache (axis 4 of the 4-axis
// paint-pipeline rewrite series).
//
// `FragmentCache` is a per-window-slot cache of encoded `vello::Scene`
// fragments keyed off `Scene::Container::paint_hash` (atomic 0). The
// shell's per-window paint cycle calls `to_vello_cached` instead of
// `to_vello`; cacheable Container subtrees skip fresh encode when their
// structural hash hits a previously stored fragment. The §2 #4
// immediate-mode coexistence enabler: an `ImmediateModeNode` sibling
// that ticks every paint frame triggers a global `V::view` re-run, but
// the retained Container subtrees keep paint-hash identical so the
// `vello::Scene::append`-from-cache path replays their encoded paint
// without re-walking every primitive.
// ---------------------------------------------------------------------------

/// R682 §5.16 — per-window paint-fragment cache for the §5.16
/// dirty-subtree-cache substrate (axis 4 of the 4-axis paint-pipeline
/// rewrite series).
///
/// Stores encoded [`VelloScene`] fragments keyed off the
/// [`pinion_core::scene::ContainerNode::paint_hash`] structural hash.
/// The cache is consulted by [`to_vello_cached`] at every cacheable
/// [`Scene::Container`] boundary, whatever transform the walk inherited
/// (R1520 — see "Fragments are transform-free" below):
///
/// - **Hit** — place the cached fragment via [`VelloScene::append`]
///   into the destination scene, pre-multiplying the inherited
///   `transform` (`None` at [`Affine::IDENTITY`], so the common case
///   encodes exactly as it did pre-R1520). The recursive walk into the
///   container's children is skipped entirely: the cache replay covers
///   them.
/// - **Miss** — encode this container's contribution into a fresh
///   sub-scene under `IDENTITY`, recursively encode children (which
///   may themselves hit the cache for nested Containers), append the
///   sub-scene to the destination under the inherited transform, and
///   insert the `IDENTITY`-encoded sub-scene into the cache under the
///   container's hash.
///
/// ## Eviction (mark-and-sweep)
///
/// Each [`to_vello_cached`] invocation brackets the encoder walk with
/// [`Self::begin_paint`] / [`Self::end_paint`]; the begin clears the
/// per-paint "seen" set, every hit / insert marks the hash, and the
/// end drops entries the most recent frame did not paint. Memory
/// bounds itself to the set of cacheable Containers actually painted
/// in the most recent frame — no LRU heuristic, no fixed cap.
///
/// ## The mark phase has a trace step (R1527)
///
/// R682 stated that bound correctly and then implemented a *proxy* for
/// it: `retain(|h| seen.contains(h))`, where `seen` is the set the
/// walker **consulted**. Painted and consulted are the same set only
/// while the cache is missing. The moment a container hits, the walk
/// returns without descending — so every fragment underneath it is
/// painted (its pixels are in the replayed fragment) and not consulted,
/// and the sweep reads that silence as death.
///
/// The divergence is therefore worst exactly where the cache works
/// best, and it is not a slow leak but a collapse in one frame.
/// Measured on a 1,200-row grid, one row per cacheable container:
///
/// | frame | entries | hits | misses | encode |
/// |---|---:|---:|---:|---:|
/// | first paint | 1,201 | 0 | 1,201 | 54.5 ms |
/// | one idle frame | **1** | 1 | 0 | 0.33 ms |
/// | one row changes | 1,201 | **0** | **1,201** | **17.1 ms** |
///
/// A single idle frame evicts 1,200 live fragments through the one root
/// that subsumed them, so the next content change — a selection moving
/// by one row, one cell committing an edit — re-encodes the whole grid
/// at **0 hits**, 59-103% of a 60fps budget for a 1/1200 delta.
///
/// A mark-and-sweep collector that marks its roots and never follows an
/// edge collects live objects; this had no edge to follow. Containment
/// is that edge: the `subsumes` map records, at install time, the
/// nearest cacheable descendants encoded inside each fragment, and
/// [`Self::end_paint`] traces the seen set through it before retaining.
/// A fragment is live if the walk consulted it **or** a live fragment
/// subsumes it, which is what "painted this frame" means.
///
/// This changes no policy. R1520 registered the eviction as a debt with
/// three candidate fixes — grace frames, an LRU with a byte budget, or
/// descending past a hit to mark — and each trades away something R682
/// chose deliberately (the absent cap, or the short-circuit that is the
/// cache's whole benefit). None is needed: the invariant in the
/// paragraph above was always the right one, and tracing is what makes
/// the code compute it. Memory stays bounded by the painted set, the
/// short-circuit stays intact, and no frame count or byte cap appears.
///
/// ## An idle frame proves its own sweep is a no-op (R1527.1)
///
/// The trace is O(live set), and the frame that most needs the fragments
/// kept alive is the one with the least reason to recompute which they
/// are. Measured before the early return below, on a grid of one
/// cacheable container per row, a *single idle frame* spent 3.3µs at 40
/// rows, 105µs at 1,200 and 335µs at 4,000 — essentially the whole frame,
/// on bookkeeping, having painted one cached fragment. That is a cost
/// this cache did not have before the trace, and it scales with how well
/// the cache is working.
///
/// It is also entirely avoidable, because such a frame can prove the
/// sweep would change nothing: the `subsumes` map is written only by
/// the miss arm, so a paint that installed nothing left the
/// containment forest untouched, and if it consulted exactly what the
/// previous paint consulted then the closure is the same closure — which
/// the previous sweep already retained. Both halves are needed. Zero
/// misses alone is not sufficient: a frame can install nothing and still
/// orphan a subtree by consulting somewhere *else* (paint one row of a
/// grid as the whole scene and it hits, while the root above it and its
/// siblings become unreachable). The comparison costs one integer and a
/// set that holds one element on exactly the frames this protects.
///
/// ## Fragments are transform-free (R1520 — closes the R682+1 carry)
///
/// The cache key is the container's `paint_hash` ALONE, and that is a
/// *complete* key because **a stored fragment never contains the
/// transform it was placed under**: the miss arm always encodes at
/// [`Affine::IDENTITY`] and the inherited transform is applied by
/// [`VelloScene::append`], which pre-multiplies every transform in the
/// appended encoding (including glyph-run transforms — see
/// `vello_encoding::Encoding::append`). Placing an `IDENTITY`-encoded
/// fragment under `T` is therefore *identical* to threading `T` through
/// the walk, because every paint site composes the inherited transform
/// on the left (`T * local`) and `T * (IDENTITY * local) == T * local`.
/// That left-composition is the invariant the equivalence rests on;
/// R1520 verified it holds at every paint site in this module.
///
/// R682 shipped the first cut with the boundary constrained to
/// `IDENTITY` — sound, because it made the transform a non-axis by
/// forbidding it, and its own doc registered the lift as a carry. What
/// the constraint cost was never measured until R1520: a
/// [`Scene::Scroll`]'s content carries a non-identity translation, so
/// **every scrolled subtree re-encoded from scratch every frame**.
/// Measured on a 4,681-node box tree, one paint of unchanged content:
/// 22µs bare, **1,360µs wrapped in a `Scroll` at a non-zero offset** —
/// and identical whether the cache was warm or cold, i.e. the cache was
/// inoperative. The shape of that cliff is what makes it worth closing:
/// caching worked while the list sat at offset 0 and stopped at the
/// first scrolled pixel, so it was absent from exactly the frames a
/// pro-tool workload spends its time in (`virtual_list` — asset
/// browser, scene outliner, data grid, log view — is a `ScrollNode`).
///
/// Two fragments with equal `paint_hash` under different transforms now
/// legitimately *share* one entry, which is a dedup rather than an
/// alias: they encode the same content and each placement supplies its
/// own transform at append time.
///
/// ## Cache-poisoning guards
///
/// - [`Scene::is_cacheable_for_paint`] rejects subtrees containing
///   [`Scene::ImmediateModeNode`] or [`Scene::External`] descendants
///   — their paint changes every frame / is opaque, so a cached
///   fragment would be stale.
/// - The `fill_hook` parameter to [`to_vello_cached`] MUST be
///   structurally derived (a function of `BoxNode` fields only, not
///   of external state). All production shell call sites pass
///   `&|_| None` which trivially satisfies this contract. Bindings
///   that need state-dependent fills should encode them into
///   `BoxStyle::fill` directly (which participates in `paint_hash`)
///   or use [`to_vello`] (non-cached) for the debug-introspect path.
#[derive(Default)]
pub struct FragmentCache {
    /// Encoded `vello::Scene` fragments keyed by container paint hash.
    fragments: HashMap<u64, VelloScene>,
    /// Hashes consulted (hit or inserted) during the current paint
    /// pass — populated between [`Self::begin_paint`] and
    /// [`Self::end_paint`], traced through [`Self::subsumes`] and swept
    /// at end.
    seen_this_paint: HashSet<u64>,
    /// R1527 §5.16 — containment edges: the nearest cacheable
    /// descendants encoded inside each stored fragment, recorded on the
    /// miss that installed it. The trace step of the mark-and-sweep —
    /// see the type doc's "The mark phase has a trace step".
    ///
    /// Keyed and valued by `paint_hash`, so the map costs 8 bytes per
    /// edge against the encoded `vello::Scene` values it keeps alive.
    /// Two containers with an equal hash have equal content and so
    /// equal child hashes, which is why a shared entry may be
    /// overwritten by either without the edge set changing.
    subsumes: HashMap<u64, Vec<u64>>,
    /// R1527 §5.16 — one frame per miss-encode currently in progress,
    /// collecting the hashes consulted inside it. Pushed by
    /// [`Self::enter_subtree`], popped by [`Self::insert_miss`] into
    /// [`Self::subsumes`]; the pairing is what makes a frame's contents
    /// exactly "the fragments encoded inside this container".
    ///
    /// Empty between paints — the walk unwinds every frame it pushes.
    child_stack: Vec<Vec<u64>>,
    /// R1527.1 §5.16 — the previous paint's [`Self::seen_this_paint`],
    /// swapped in by [`Self::begin_paint`]. Held so [`Self::end_paint`]
    /// can recognise a frame that cannot evict anything and skip the
    /// trace entirely; see the "an idle frame proves its own sweep is a
    /// no-op" section of the type doc.
    seen_last_paint: HashSet<u64>,
    /// R1527.1 §5.16 — [`Self::misses`] as of the matching
    /// [`Self::begin_paint`], so `end_paint` can ask whether THIS paint
    /// installed anything without a second counter to keep in step.
    misses_at_begin: u64,
    /// R682 §5.16 atomic 2 — damage region accumulator for the
    /// current paint pass. Each cache-miss container's rect unions
    /// in; ends as the per-paint [`Self::last_damage_region`] published
    /// at [`Self::end_paint`].
    ///
    /// `None` while no miss has happened yet during the paint;
    /// `Some(_)` after the first miss. The empty-rect short-circuit
    /// inside [`Rect::union`] keeps zero-area cache entries from
    /// extending the region beyond their actual painted footprint.
    damage_acc_this_paint: Option<pinion_core::scene::Rect>,
    /// R682 §5.16 atomic 2 — most-recent-paint damage region. The
    /// union of every cache-miss Container's `rect` during the last
    /// completed paint pass. `None` when the last paint was 100%
    /// cache-hit (no visual delta from the previous frame).
    ///
    /// The current pinion-shell paint cycle resets the entire
    /// `vello::Scene` and submits a full surface every frame, so
    /// this value is observability-only — a future round that wires
    /// `wgpu::SurfaceTexture` partial-blit / `winit` damage-rect
    /// `WindowEvent::RedrawRequested` interaction consumes it as
    /// the actual GPU upload bounds.
    last_damage_region: Option<pinion_core::scene::Rect>,
    /// Observability counters — cumulative across the cache lifetime
    /// (per-window). Exposed via [`Self::hits`] / [`Self::misses`] /
    /// [`Self::hit_rate`] for the R682 demo / future RPC stats wire.
    hits: u64,
    /// See [`Self::hits`].
    misses: u64,
    /// Number of [`Self::end_paint`] calls — the per-window paint
    /// pass counter for `hit_rate` denominator interpretation.
    paint_count: u64,
    /// R1538 §5.16 — scene nodes the encode walk has entered during the
    /// current paint pass. Reset by [`Self::begin_paint`], published into
    /// [`Self::nodes_last_paint`] by [`Self::end_paint`].
    nodes_this_paint: u32,
    /// R1538 §5.16 — the completed paint's node census: how many `Scene`
    /// nodes the encoder actually walked.
    ///
    /// Distinct from [`Self::hits`] / [`Self::misses`], which count cacheable
    /// *containers* consulted. This counts every node entered, and a hit
    /// returns before descending — so the ratio against the painted tree's
    /// size is what says the cache did its job on THIS frame. A hit rate
    /// cannot: replaying two enormous fragments and replaying two tiny ones
    /// are both 100%.
    nodes_last_paint: u32,
}

/// (R682 §5.16) Manual `Debug` because [`vello::Scene`] does not
/// implement it (encoded GPU command streams have no canonical
/// human-readable form). Surfaces the lifetime counters + entry
/// count, which is what cache observability actually needs.
impl core::fmt::Debug for FragmentCache {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // `finish_non_exhaustive` covers the per-paint
        // `damage_acc_this_paint` accumulator (intentionally elided —
        // the public observable is `last_damage_region`, published at
        // `end_paint` from the accumulator).
        f.debug_struct("FragmentCache")
            .field("entries", &self.fragments.len())
            .field("seen_this_paint", &self.seen_this_paint.len())
            .field("hits", &self.hits)
            .field("misses", &self.misses)
            .field("paint_count", &self.paint_count)
            .field("last_damage_region", &self.last_damage_region)
            .finish_non_exhaustive()
    }
}

impl FragmentCache {
    /// Construct an empty cache with zero counters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a new paint pass. Clears the per-paint "seen" set so
    /// [`Self::end_paint`] can sweep unreferenced entries afterwards,
    /// and resets the per-paint damage accumulator
    /// ([`Self::last_damage_region`] is published at
    /// [`Self::end_paint`] from this accumulator).
    pub fn begin_paint(&mut self) {
        // R1527.1 — this paint's set becomes the previous one. The swap
        // reuses both allocations, so remembering the last frame costs a
        // pointer move rather than a clone.
        std::mem::swap(&mut self.seen_this_paint, &mut self.seen_last_paint);
        self.seen_this_paint.clear();
        self.misses_at_begin = self.misses;
        self.damage_acc_this_paint = None;
        // R1538 — the census is per paint, not cumulative: the question it
        // answers ("how much of the tree did THIS frame walk?") has no
        // lifetime form. A cumulative counter differenced by the caller would
        // give the same number and put the pairing in the caller's hands.
        self.nodes_this_paint = 0;
        // Defensive: the walk pops every frame it pushes, so this is
        // already empty. Clearing anyway keeps a panic unwinding out of
        // a paint from leaking a frame into the next one, where it would
        // silently attribute the next frame's fragments to a container
        // that is no longer being encoded.
        self.child_stack.clear();
    }

    /// Close the current paint pass. Drops cache entries the frame did
    /// not paint; increments the paint counter; publishes the per-paint
    /// damage accumulator into [`Self::last_damage_region`].
    ///
    /// R1527 — "did not paint" is the seen set closed over
    /// the `subsumes` containment map, not the seen set itself. A fragment
    /// the walk never consulted because a hit above it short-circuited the
    /// descent is painted, not dead; see the type doc.
    ///
    /// The closure is a worklist trace over the containment forest, so
    /// it visits each live fragment once — O(live entries), no deeper
    /// than the container nesting, and it cannot loop: `insert` gates
    /// re-entry, which also makes the shared entry two equal-hash
    /// containers may produce harmless.
    pub fn end_paint(&mut self) {
        // R1538 — the per-paint publishes, hoisted ahead of the early return
        // below. None of them depends on the sweep, and before R1538 each of
        // the two exits carried its own copy of the pair — which is exactly
        // how a third one comes to be written on one path only. Now there is
        // one copy and the early return is purely about eviction.
        self.paint_count = self.paint_count.saturating_add(1);
        self.last_damage_region = self.damage_acc_this_paint;
        self.nodes_last_paint = self.nodes_this_paint;
        // R1527.1 — a frame that installed nothing and consulted exactly
        // what the last frame consulted cannot have orphaned anything:
        // `subsumes` only changes on a miss, so the closure is the same
        // closure, and the previous sweep already retained exactly it.
        //
        // This is the idle frame, and it is where skipping matters most,
        // because the trace is O(live set) while `seen` is O(1) — the
        // root alone. Measured on a grid of one cacheable container per
        // row, `end_paint` before this early return: 3.3us at 40 rows,
        // 105us at 1,200, 335us at 4,000 — pure bookkeeping on a frame
        // that painted one cached fragment. The comparison that replaces
        // it is one integer and a one-element set.
        if self.misses == self.misses_at_begin && self.seen_this_paint == self.seen_last_paint {
            return;
        }
        let mut live: HashSet<u64> = self.seen_this_paint.iter().copied().collect();
        let mut work: Vec<u64> = live.iter().copied().collect();
        while let Some(hash) = work.pop() {
            let Some(children) = self.subsumes.get(&hash) else {
                continue;
            };
            for &child in children {
                if live.insert(child) {
                    work.push(child);
                }
            }
        }
        self.fragments.retain(|hash, _| live.contains(hash));
        // The edges of a dropped fragment describe an encode that no
        // longer exists. Retaining them would keep a dead container's
        // children reachable from nothing, and grow without bound.
        self.subsumes.retain(|hash, _| live.contains(hash));
    }

    /// R1538 §5.16 — how many `Scene` nodes the encode walk entered during the
    /// last completed paint.
    ///
    /// The paint-side half of the frame's node census; the build-side half is
    /// [`LayoutPass::nodes`](crate::LayoutPass::nodes). Read together they say
    /// what neither says alone: how big the painted tree is, and how much of
    /// it this frame had to re-encode.
    ///
    /// Not folded into [`FragmentCacheStats`] — that snapshot is
    /// `scene/cache_stats`'s axis (hit rate), and this belongs to the frame's
    /// cost, which is `scene/frame_timings`. One observability axis per
    /// method.
    #[must_use]
    pub fn nodes_walked_last_paint(&self) -> u32 {
        self.nodes_last_paint
    }

    /// R1538 §5.16 — count one node entered by the encode walk.
    ///
    /// Called at the top of the walker, before the cacheable fast path, so a
    /// hit's own container counts (the walk did visit it) while the subtree it
    /// short-circuits does not (the walk did not).
    fn note_node(&mut self) {
        self.nodes_this_paint = self.nodes_this_paint.saturating_add(1);
    }

    /// R682 §5.16 atomic 2 — damage region from the most recent
    /// completed paint pass. The bounding rect of every
    /// [`Scene::Container`] that cache-missed (== whose paint output
    /// MAY differ from the previous paint's same-position output).
    ///
    /// `None` when the last paint was 100% cache-hit (no visual
    /// delta from the previous frame; the surface texture is
    /// pixel-identical to the previous submit).
    ///
    /// Current consumers: tests + R682 demo + future RPC stats
    /// wire. Production GPU consumer (`wgpu::SurfaceTexture`
    /// partial-blit / `winit` damage-rect coordination) is a
    /// follow-up round carry — the current per-window paint cycle
    /// resets the entire `vello::Scene` and submits a full surface
    /// every frame regardless.
    #[must_use]
    pub fn last_damage_region(&self) -> Option<pinion_core::scene::Rect> {
        self.last_damage_region
    }

    /// Cumulative cache hits across this cache's lifetime.
    #[must_use]
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Cumulative cache misses across this cache's lifetime.
    #[must_use]
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// Cumulative paint passes (== [`Self::end_paint`] invocations)
    /// across this cache's lifetime.
    #[must_use]
    pub fn paint_count(&self) -> u64 {
        self.paint_count
    }

    /// `hits / (hits + misses)`. Returns `0.0` when no lookup has
    /// happened yet (avoids `0/0` `NaN`).
    #[must_use]
    pub fn hit_rate(&self) -> f32 {
        let total = self.hits.saturating_add(self.misses);
        if total == 0 {
            return 0.0;
        }
        // The conversion is lossy past 2^24, but for our counters
        // (paint counts; even a 144-fps day saturates well below
        // that) the ratio is faithful well into the noise.
        #[allow(
            clippy::cast_precision_loss,
            reason = "hit_rate is a debug telemetry ratio, not numeric pipeline input"
        )]
        {
            self.hits as f32 / total as f32
        }
    }

    /// Number of cached fragments currently held. The mark-and-sweep
    /// eviction strategy bounds this to the cacheable Container set
    /// painted in the most recent frame, so the count == "live
    /// cacheable Container tree node count" after the first sweep.
    #[must_use]
    pub fn entries(&self) -> usize {
        self.fragments.len()
    }

    /// Reset every lifetime counter + drop every cached fragment.
    /// Used by tests / future RPC `cache/reset` to set a clean
    /// baseline.
    pub fn clear(&mut self) {
        self.fragments.clear();
        self.seen_this_paint.clear();
        self.damage_acc_this_paint = None;
        self.last_damage_region = None;
        self.hits = 0;
        self.misses = 0;
        self.paint_count = 0;
    }

    /// R682.B §5.16 — capture the observable counters + entry count +
    /// damage region as a GUI-agnostic value-type snapshot.
    ///
    /// The returned [`FragmentCacheStats`] holds no `vello::Scene`
    /// references, so consumers in non-vello build profiles (TUI /
    /// headless tests / `pinion-rpc::DispatchContext`) can hold the
    /// snapshot without dragging in the GPU stack. Called once per
    /// paint cycle from `pinion-shell::AppShell::render_window` after
    /// `to_vello_cached` returns, and from R682.B tests that drive
    /// `to_vello_cached` directly without a winit surface.
    #[must_use]
    pub fn stats(&self) -> FragmentCacheStats {
        FragmentCacheStats {
            hits: self.hits,
            misses: self.misses,
            paint_count: self.paint_count,
            entries: self.fragments.len(),
            last_damage_region: self.last_damage_region,
        }
    }

    /// Probe + replay path. When `hash` is in the cache, place the
    /// stored fragment into `out` under `transform` (R1520) and mark the
    /// hash as seen. Every stored fragment is encoded under
    /// [`Affine::IDENTITY`], so the inherited transform is supplied here
    /// rather than baked in — see the type doc's "Fragments are
    /// transform-free".
    ///
    /// Returns `true` on hit, `false` on miss. Bumps the hit counter
    /// on hit; the miss counter is bumped by [`Self::insert_miss`]
    /// to keep the increment paired with the actual cache install.
    fn try_hit(&mut self, hash: u64, out: &mut VelloScene, transform: Affine) -> bool {
        if let Some(fragment) = self.fragments.get(&hash) {
            append_placed(out, fragment, transform);
            self.mark_painted(hash);
            self.hits = self.hits.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// R1527 §5.16 — record that `hash` was painted this pass: mark it
    /// seen, and attribute it to the miss-encode currently in progress
    /// (if any) as one of that container's nearest cacheable
    /// descendants.
    ///
    /// "Nearest" falls out of the stack rather than being computed: a
    /// hit does not descend and a miss pushes its own frame, so the
    /// innermost open frame always belongs to the closest enclosing
    /// cacheable container, whatever non-cacheable nodes
    /// ([`Scene::Scroll`], [`Scene::Box`]) the walk crossed to get here.
    fn mark_painted(&mut self, hash: u64) {
        self.seen_this_paint.insert(hash);
        if let Some(frame) = self.child_stack.last_mut() {
            frame.push(hash);
        }
    }

    /// R1527 §5.16 — open a frame to collect the fragments encoded
    /// inside one container's subtree. Called on the miss arm before
    /// the walk descends; [`Self::insert_miss`] closes it.
    ///
    /// Every caller reaches `insert_miss` unconditionally after
    /// descending, which is what keeps the stack balanced — see the
    /// cacheable-container arm of [`to_vello_cached_inner`].
    fn enter_subtree(&mut self) {
        self.child_stack.push(Vec::new());
    }

    /// Install a freshly encoded fragment under `hash` and mark it as
    /// seen this paint. The miss counter is bumped here (paired with
    /// the actual encode, not just a probe). The container's `rect`
    /// contributes to the per-paint damage accumulator (R682 atomic 2):
    /// a missed Container's bounds is where the painted output may
    /// differ from the previous frame.
    ///
    /// R1520 — `rect` arrives in the container's own frame and the
    /// accumulator publishes *screen* space, so `transform` is applied
    /// here: a consumer of [`Self::last_damage_region`] uploads pixels,
    /// and a scrolled container's own rect is not where its pixels land.
    /// The mapping lives with the accumulator rather than at the call
    /// site so the two cannot drift into different coordinate spaces.
    ///
    /// The transform is the container's **screen placement**, not the
    /// transform its fragment was encoded under: inside a cached fragment
    /// the encode transform is `IDENTITY` while the fragment as a whole is
    /// still going to land somewhere, and a nested container's damage has
    /// to say where its pixels are, not where they are stored.
    ///
    /// `clip` is the accumulated clip the walk reached this container under
    /// (`None` = unclipped), and the placed rect is intersected with it. A
    /// clipped container paints nothing outside its clip, so nothing outside
    /// it can differ — the intersection is unconditionally correct and
    /// strictly tighter, with no policy for a consumer to choose.
    ///
    /// It is also what keeps letting the cache reach scrolled subtrees from
    /// making this field WORSE than R682 left it. A scrolled container's own
    /// rect is its full *content* extent, so an unclipped 10,000-row list
    /// reported a 320,000px-tall damage region inside a 460px window. Clipped,
    /// it is bounded by the viewport — tighter than the whole-window rect R682
    /// published, rather than 700x looser.
    ///
    /// What remains over-covers and is meant to: a missed container reports
    /// its whole clipped rect, not the sub-area whose pixels actually moved.
    /// A damage region that under-covers is a stale-pixel bug; one that
    /// over-covers is a wasted upload.
    fn insert_miss(
        &mut self,
        hash: u64,
        fragment: VelloScene,
        rect: pinion_core::scene::Rect,
        placement: Affine,
        clip: Option<pinion_core::scene::Rect>,
    ) {
        self.fragments.insert(hash, fragment);
        // R1527 — close this container's frame first: it holds the
        // fragments encoded *inside* it, and the container itself
        // belongs to its parent's frame, which `mark_painted` reaches
        // only once this one is popped.
        let children = self.child_stack.pop().unwrap_or_default();
        // R1527.1 — a leaf fragment subsumes nothing, and most fragments
        // in a real scene are leaves (1,200 of the 1,201 in a row grid).
        // Storing an edge set for them costs a map entry each and the
        // trace reads it only to find it empty. `remove` rather than
        // "skip the insert" so the map cannot keep a stale non-empty set
        // under a hash that has since become a leaf.
        if children.is_empty() {
            self.subsumes.remove(&hash);
        } else {
            self.subsumes.insert(hash, children);
        }
        self.mark_painted(hash);
        self.misses = self.misses.saturating_add(1);
        let placed = match clip {
            Some(c) => intersection(screen_bounds(rect, placement), c),
            None => screen_bounds(rect, placement),
        };
        self.damage_acc_this_paint = Some(match self.damage_acc_this_paint {
            Some(acc) => acc.union(placed),
            None => placed,
        });
    }
}

/// (R1520 §5.16) Place an `IDENTITY`-encoded fragment into `out` under
/// `transform`.
///
/// `None` is passed at [`Affine::IDENTITY`] rather than
/// `Some(Affine::IDENTITY)`: `vello_encoding::Encoding::append` takes a
/// `map`-over-every-transform branch when a transform is supplied and a
/// flat `extend_from_slice` when it is not, so the identity case stays on
/// the byte-for-byte pre-R1520 path instead of paying a multiply per
/// transform-stream entry to arrive at the same numbers.
fn append_placed(out: &mut VelloScene, fragment: &VelloScene, transform: Affine) {
    if transform == Affine::IDENTITY {
        out.append(fragment, None);
    } else {
        out.append(fragment, Some(transform));
    }
}

/// (R1520 §5.16) The overlap of two rects; empty (`w == 0 || h == 0`) when
/// they do not meet.
///
/// A local peer of [`pinion_core::scene::Rect::union`] rather than an addition
/// to `Rect`'s surface: the damage accumulator is the only caller, and
/// `pinion-core` already carries the intersection *predicate*
/// (`rects_intersect`) that hit-testing wants. A second consumer is what would
/// make this a `Rect` method.
fn intersection(
    a: pinion_core::scene::Rect,
    b: pinion_core::scene::Rect,
) -> pinion_core::scene::Rect {
    let lx = a.x.max(b.x);
    let ty = a.y.max(b.y);
    let rx = a.x.saturating_add(a.w).min(b.x.saturating_add(b.w));
    let by = a.y.saturating_add(a.h).min(b.y.saturating_add(b.h));
    pinion_core::scene::Rect::new(lx, ty, rx.saturating_sub(lx), by.saturating_sub(ty))
}

/// (R1520 §5.16) `rect`'s axis-aligned bounding box after `transform`,
/// clamped into the unsigned [`pinion_core::scene::Rect`] space.
///
/// All four corners are mapped, not just the origin: an
/// [`Affine`] may rotate or skew, and the bounding box of the mapped
/// corners is the only answer that stays *conservative* (a damage region
/// that under-covers is a stale-pixel bug, one that over-covers is only
/// wasted upload). Pure translation — every transform the current walk
/// produces — reduces to the exact translated rect.
///
/// Negative coordinates clamp to `0` while the far edge is preserved, so
/// a container scrolled above the viewport keeps reporting the part that
/// is still on screen. `Rect` is unsigned by §5.2, so the off-screen
/// half cannot be represented and is not needed: it paints nowhere.
fn screen_bounds(rect: pinion_core::scene::Rect, transform: Affine) -> pinion_core::scene::Rect {
    let x0 = f64::from(rect.x);
    let y0 = f64::from(rect.y);
    let x1 = x0 + f64::from(rect.w);
    let y1 = y0 + f64::from(rect.h);
    let corners =
        [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| transform * KurboPoint::new(x, y));
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in corners {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    let clamp = |v: f64| -> u32 {
        // `as` on a f64 → u32 saturates at both ends in Rust, and NaN maps
        // to 0; the explicit floor / max keeps the intent readable.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to the u32 range on both ends immediately below"
        )]
        {
            v.max(0.0).min(f64::from(u32::MAX)).floor() as u32
        }
    };
    let (lx, ly) = (clamp(min_x), clamp(min_y));
    pinion_core::scene::Rect::new(
        lx,
        ly,
        clamp(max_x).saturating_sub(lx),
        clamp(max_y).saturating_sub(ly),
    )
}

/// R682 §5.16 — cached counterpart to [`to_vello`].
///
/// Walks the [`Scene`] tree like [`to_vello`] but consults the
/// supplied [`FragmentCache`] at every cacheable
/// [`Scene::Container`] boundary it reaches, at any accumulated
/// transform (R1520) — a cache hit places the previously encoded
/// `vello::Scene` fragment into `out` under that transform and skips the
/// recursive walk into children; a miss encodes the subtree fresh into a
/// sub-scene, places it in `out`, and stores it in the cache for the next
/// paint.
///
/// Brackets the encoder walk with [`FragmentCache::begin_paint`] /
/// [`FragmentCache::end_paint`] so unreached cache entries are
/// evicted at the end of the call. Callers do NOT need to manage
/// begin/end themselves; this is the single-call API for the shell's
/// per-window paint cycle.
///
/// The two paint-time terminal-cursor flags, threaded together through the
/// Vello walk so the pair cannot be swapped or drift apart: `blink_on` is the
/// R1426 blink PHASE (true on the visible half) and `focused` is the R1427
/// OS-focus state (true = filled, false = the unfocused hollow box). Bundling
/// them keeps [`to_vello_cached_inner`]'s recursion a single trailing argument;
/// the public entries keep the two bools and build this internally.
#[derive(Clone, Copy)]
struct CursorPaintFlags {
    blink_on: bool,
    focused: bool,
}

impl CursorPaintFlags {
    /// The deterministic default the uncached reference / headless / produce
    /// entries force: a steady ON phase on a focused window (a filled cursor),
    /// so a golden screenshot never flakes on the wall-clock phase or a live
    /// OS-focus fact those stateless paths do not carry.
    const STEADY_FOCUSED: Self = Self {
        blink_on: true,
        focused: true,
    };
}

/// (R1556 §5.16) How many entries one `push_clip_layer` … `pop_layer` pair
/// contributes to the encoder's clip counter.
///
/// `vello_encoding` bumps `n_clips` on the begin **and** on the end, so a
/// balanced layer costs two. That is an implementation detail of a crate this
/// project does not own — exactly the model-drift class R1550 recorded, where a
/// number stays green while the crate underneath it changes shape — so it is
/// named here and **pinned by a test** (`r1556_a_layer_costs_two_clip_entries`)
/// rather than spelled inline as a `2`. If vello changes the accounting, that
/// test fails; nothing else in this file would notice.
const CLIP_ENTRIES_PER_LAYER: u32 = 2;

/// (R1556 §5.16) The [`DrawWork`] census of an encoded scene: what this frame
/// will actually ask the renderer to draw.
///
/// Read from the encoding rather than accumulated during the walk, and that is
/// the whole point. The §5.16 fragment cache serves a hit by **appending** a
/// stored fragment, and `vello`'s append extends the encoded streams and folds
/// the appended counters in — so a replayed subtree lands in these numbers
/// identically to a freshly-encoded one. A tally kept by the walker would count
/// only what it walked, which is [`crate::FrameTiming::encode_nodes`]' job and the
/// opposite question.
///
/// Call it on the scene that is **submitted**, not on the pre-scale one: the DPI
/// append copies the streams verbatim, so the counts are scale-invariant either
/// way, and measuring the submitted scene is what makes "this is what ran" true
/// by construction rather than by review.
///
/// # Layers
///
/// Derived as `(n_clips + n_open_clips) / 2`, which is
/// **exact** rather than approximate: a begin adds one to each counter and an
/// end adds one to the first while taking one off the second, so for `b` begins
/// and `e` ends the sum is `(b + e) + (b - e) = 2b`. A frame that left a layer
/// open therefore still reports the layer it pushed, instead of truncating it
/// away. The `2` is the one quantity here that MODELS a crate this project
/// does not own; it is named and pinned by a test rather than spelled inline —
/// see the private `CLIP_ENTRIES_PER_LAYER` in this module.
#[must_use]
pub fn draw_work_of(scene: &VelloScene) -> DrawWork {
    let enc = scene.encoding();
    DrawWork {
        draws: saturating_count(enc.draw_tags.len()),
        paths: enc.n_paths,
        path_segments: enc.n_path_segments,
        layers: enc
            .n_clips
            .saturating_add(enc.n_open_clips)
            .saturating_div(CLIP_ENTRIES_PER_LAYER),
        glyph_runs: saturating_count(enc.resources.glyph_runs.len()),
        glyphs: saturating_count(enc.resources.glyphs.len()),
    }
}

/// A stream length as a census field. Saturates rather than wrapping: a scene
/// with more than four billion glyphs in one frame is not a number worth
/// preserving exactly, but it must not be reported as a small one.
fn saturating_count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// `fill_hook` is honoured for [`Scene::Box`] leaves (matching
/// [`to_vello`]'s contract). For the cache to remain correct, the
/// hook MUST be structurally derived (a function of the
/// [`BoxNode`] fields only, not external state) — see
/// [`FragmentCache`] for the rationale.
pub fn to_vello_cached<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    image_cache: &mut ImageCache,
    fragment_cache: &mut FragmentCache,
    out: &mut VelloScene,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    // R1072 §5.37 — the engine-free cached walk: forwards `None`, so `Scene::Text`
    // paints via parley exactly as before (byte-identical to the pre-R1072 body).
    // R1426 §5.41 — forwards the STEADY blink phase (`cursor_blink_on = true`): this
    // thin wrapper is the ~30-call-site test / non-animating entry, so a TextGrid
    // cursor always paints (a blinking-mode cursor renders on its visible phase).
    // The live winit surface calls `to_vello_cached_with_text_engine` directly with
    // the per-window clock's phase; only that path animates.
    // R1427 §5.41 — likewise forwards the FOCUSED default (`cursor_focused = true`):
    // this stateless entry has no per-window OS-focus fact, so the cursor renders
    // filled (the live winit surface passes the window's real focus).
    to_vello_cached_with_text_engine(
        scene,
        fill_hook,
        text_cache,
        image_cache,
        fragment_cache,
        None,
        out,
        true,
        true,
    );
}

/// R1072 §5.37 → production `Scene::Text` — the cached production paint walker
/// ([`to_vello_cached`]) with the opt-in self-hosted text engine.
///
/// This is the cached sibling of [`to_vello_with_text_engine`]: the shell's
/// per-window paint cycle uses the [`FragmentCache`], so wiring the engine into
/// production needs the engine to flow through the cached walker too (R1068 added
/// the uncached arm only; this closes the cached gap). When `engine` is `Some`,
/// every eligible `self_hosted_eligible` `Scene::Text` leaf the walk reaches —
/// inside a freshly-encoded subtree or a cache miss — paints through §5.37;
/// `engine = None` is byte-identical to the pre-R1072 [`to_vello_cached`].
///
/// CACHE COHERENCE: the engine is a per-window, per-cache-lifetime constant (the
/// shell builds it once at init, gated by `PINION_TEXT_ENGINE`), so a fragment
/// cached while the engine is enabled stays valid for replay — the engine choice
/// never changes mid-session. The leaf's caret-bearing marker is folded into the
/// `Scene::Text` paint hash ([`pinion_core::scene::Scene::paint_hash`]) so two
/// leaves differing only in which shaper paints them never share a fragment.
///
/// WIRE BOTH ARMS OR NEITHER: pair this with the measure override
/// [`compute_layout_with_text_measure`](crate::layout::compute_layout_with_text_measure)
/// using the SAME engine (see that fn's contract).
///
/// R1426 §5.41 §5.28 — `cursor_blink_on` is the render-time terminal-cursor
/// blink PHASE for this frame (a per-window free-running clock, driven live by
/// the shell): `true` on the visible half, `false` on the hidden half. It gates
/// only a `TextGrid` cursor in the blinking DECSCUSR mode
/// ([`GridCursor::shown_this_phase`](pinion_core::term_grid::GridCursor::shown_this_phase));
/// a steady cursor ignores it. It is a paint-time argument — the phase is never
/// stored in the scene, so `scene/snapshot` stays stable (§2 #7). The live
/// winit paint passes the clock's phase; headless / produce / test callers pass
/// the steady default (`true`) so a cursor renders deterministically in its ON
/// phase (no wall-clock flake in a golden screenshot).
///
/// R1427 §5.41 §5.39 — `cursor_focused` is whether the window being painted holds
/// the OS keyboard focus (the shell's `is_key_dispatch_window`, fails OPEN when
/// focus is unknown). `false` draws a visible cursor as a HOLLOW outline box
/// (overriding shape) — the universal unfocused-terminal indicator; `true` is the
/// filled render. Like `cursor_blink_on` it is a paint-time argument never stored
/// in the scene (§2 #7 — OS focus is already data via `scene/input_state`). The
/// live winit surface passes the window's focus; headless / produce / test callers
/// pass the focused default (`true`) so a golden screenshot renders the filled
/// cursor deterministically.
#[allow(clippy::too_many_arguments)]
pub fn to_vello_cached_with_text_engine<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    image_cache: &mut ImageCache,
    fragment_cache: &mut FragmentCache,
    engine: Option<&SelfHostedTextEngine>,
    out: &mut VelloScene,
    cursor_blink_on: bool,
    cursor_focused: bool,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    walk_cached(
        scene,
        &mut PaintSession {
            fill_hook,
            text_cache,
            image_cache,
            fragment_cache,
            engine,
            cursor: CursorPaintFlags {
                blink_on: cursor_blink_on,
                focused: cursor_focused,
            },
            profiler: None,
        },
        out,
    );
}

/// (R1557 §5.16 §2 #2 §2 #7) [`to_vello_cached_with_text_engine`] with a
/// [`DrawProfiler`] attached: the same walk, additionally attributing the
/// frame's [`DrawWork`] to the subtree that drew it.
///
/// The profiler is handed a census of `out` on the way into every node and
/// another on the way out, so a node's inclusive cost is the difference —
/// including the cost of a subtree the [`FragmentCache`] **replayed**, which
/// arrives as an `append` and is therefore in the difference exactly as a
/// freshly-encoded one would be. See [`crate::draw_profile`] for why that is the
/// only defensible way to attribute a retained-mode frame.
///
/// # Cost, and why this is a separate entry
///
/// Two [`draw_work_of`] reads and one `Vec` push per walked node. That is
/// cheap, but it is not free, and a profiler that bills every frame for the
/// measurement nobody asked for is the failure mode profilers are famous for.
/// So it is an entry a caller opts into, and the unprofiled walk pays one
/// `Option` discriminant test per node and nothing else.
///
/// # Errors that cannot happen
///
/// The profiler's `enter`/`leave` pairing is structural rather than
/// remembered: `to_vello_cached_inner` opens the frame, delegates the ENTIRE
/// node — every arm, the cache-hit early return included — to `encode_node`,
/// and closes the frame on the one path back out.
#[allow(clippy::too_many_arguments)]
pub fn to_vello_cached_profiled<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    image_cache: &mut ImageCache,
    fragment_cache: &mut FragmentCache,
    engine: Option<&SelfHostedTextEngine>,
    out: &mut VelloScene,
    cursor_blink_on: bool,
    cursor_focused: bool,
    profiler: &mut DrawProfiler,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    walk_cached(
        scene,
        &mut PaintSession {
            fill_hook,
            text_cache,
            image_cache,
            fragment_cache,
            engine,
            cursor: CursorPaintFlags {
                blink_on: cursor_blink_on,
                focused: cursor_focused,
            },
            profiler: Some(profiler),
        },
        out,
    );
}

/// (R1557 §5.16) The state a cached paint walk carries **unchanged** from its
/// root to every leaf, in one value.
///
/// Split out when the draw profiler became the seventh such item and the
/// recursive walker's parameter list stopped being incidental: three of them
/// were `&mut` references and two were `Option`s, which is precisely the
/// signature shape where a transposed argument compiles. What varies per node —
/// the node, the output scene, the two transforms, the clip, the child index —
/// stays a parameter, because those are the walk's actual state.
struct PaintSession<'a, F> {
    fill_hook: &'a F,
    text_cache: &'a mut LayoutCache,
    image_cache: &'a mut ImageCache,
    fragment_cache: &'a mut FragmentCache,
    engine: Option<&'a SelfHostedTextEngine>,
    cursor: CursorPaintFlags,
    /// `None` on every production paint; `Some` only under
    /// [`to_vello_cached_profiled`].
    profiler: Option<&'a mut DrawProfiler>,
}

/// The shared body of both cached entries: bracket the walk in the fragment
/// cache's paint generation and start it at the root.
///
/// `index: None` at the root — the root consumes no path segment, which is the
/// `scene/locate` addressing rule the profiler mirrors.
fn walk_cached<F>(scene: &Scene, session: &mut PaintSession<'_, F>, out: &mut VelloScene)
where
    F: Fn(&BoxNode) -> Option<Color>,
{
    session.fragment_cache.begin_paint();
    to_vello_cached_inner(
        scene,
        session,
        out,
        Affine::IDENTITY,
        Affine::IDENTITY,
        None,
        None,
    );
    session.fragment_cache.end_paint();
}

/// Transform-carrying recursive walker for [`to_vello_cached`].
///
/// Mirrors [`to_vello_inner`]'s match shape but adds a cache check
/// at the top of the [`Scene::Container`] arm: when the subtree is
/// cacheable per [`Scene::is_cacheable_for_paint`], probe the cache
/// first. Hit → place the fragment under the accumulated transform,
/// return. Miss → encode the entire subtree into a fresh `VelloScene`
/// **at [`Affine::IDENTITY`]**, place it in `out` under the accumulated
/// transform, install in cache.
///
/// Non-cacheable Containers take the direct encode path: paint the
/// container's own fill into `out`, recurse into children with the
/// same transform. Recursion stays inside this function so that
/// cacheable descendant Containers (e.g. a stable widget subtree
/// sitting next to an `ImmediateModeNode` sibling — the §2 #4
/// killer case) can hit the cache even though their non-cacheable
/// parent missed.
///
/// Leaves ([`Scene::Box`], [`Scene::Text`], [`Scene::Effect`],
/// [`Scene::External`], [`Scene::Path`], [`Scene::Image`],
/// [`Scene::ImmediateModeNode`]) share their paint logic with
/// [`to_vello_inner`] — same `fill_rect` / `paint_text` /
/// `paint_path` / `paint_immediate_mode_node` helpers.
/// [`Scene::Scroll`] threads
/// the inherited transform exactly like [`to_vello_inner`]; since R1520
/// the recursion into `content` reaches a cacheable Container that caches
/// **under the scroll translation**, so a scrolled subtree is encoded once
/// and re-placed at each subsequent offset instead of re-encoded per frame.
///
/// ## Two transforms (R1520)
///
/// * `transform` — the **encoding** frame. Reset to [`Affine::IDENTITY`]
///   on entering a cached fragment, which is what makes the fragment
///   re-placeable; every paint primitive uses this one.
/// * `placement` — the **screen** frame this subtree's pixels land in.
///   Never reset. Pre-R1520 the two were the same value and one parameter
///   carried both meanings; splitting the cache boundary off `IDENTITY`
///   is what separated them.
///
/// The distinction is load-bearing exactly once, and invisibly: a
/// container nested inside a scrolled fragment recurses with `transform ==
/// IDENTITY`, so reporting its damage against `transform` would place it
/// at its content coordinates — inside the fragment, correct; on the
/// screen, off by the whole scroll offset. `placement` is what
/// [`FragmentCache::insert_miss`] is given.
///
/// ## The profiler bracket (R1557)
///
/// This function is the enter/leave bracket and delegates the whole node to
/// [`encode_node`]. Keeping the two apart is what makes the pairing structural:
/// every arm of the node — the cache hit's early return included — is inside
/// one call, so there is no path that opens a profile frame and does not close
/// it. `index` is the node's position among its parent's children, `None` for a
/// node that consumes no path segment (the root, a `Scroll`'s content); it is an
/// index rather than a formatted segment so an unprofiled paint allocates no
/// name.
fn to_vello_cached_inner<F>(
    scene: &Scene,
    session: &mut PaintSession<'_, F>,
    out: &mut VelloScene,
    transform: Affine,
    placement: Affine,
    clip: Option<pinion_core::scene::Rect>,
    index: Option<usize>,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    if session.profiler.is_none() {
        encode_node(scene, session, out, transform, placement, clip);
        return;
    }
    let before = draw_work_of(out);
    if let Some(profiler) = session.profiler.as_deref_mut() {
        profiler.enter(scene, index, before);
    }
    encode_node(scene, session, out, transform, placement, clip);
    let after = draw_work_of(out);
    if let Some(profiler) = session.profiler.as_deref_mut() {
        profiler.leave(after);
    }
}

/// One node of the cached walk: its own paint plus the recursion into whatever
/// it contains. Split from [`to_vello_cached_inner`] at R1557 so the profiler
/// bracket there wraps every arm without a per-arm hook.
fn encode_node<F>(
    scene: &Scene,
    session: &mut PaintSession<'_, F>,
    out: &mut VelloScene,
    transform: Affine,
    placement: Affine,
    clip: Option<pinion_core::scene::Rect>,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    // R1538 §5.16 — census one entered node. Ahead of every arm, including
    // the cache probe below: this counts what the WALK touched, and a hit is
    // a node the walk touched and then declined to descend through.
    session.fragment_cache.note_node();
    // Cacheable-container fast path — taken whenever the subtree
    // contains no immediate-mode / external descendants, at ANY
    // inherited transform (R1520; R682 required IDENTITY here). A hit
    // places the stored fragment; a miss encodes the subtree into a
    // fresh sub-scene and places + caches it. Either way `out` receives
    // an `append`, never a direct draw (see the R706 invariant below).
    //
    // The fragment is always encoded under IDENTITY and the inherited
    // `transform` is applied at placement, which is what keeps
    // `paint_hash` a complete cache key — see `FragmentCache`'s
    // "Fragments are transform-free". Because the recursion below re-enters
    // with IDENTITY, a nested Container inside a scrolled subtree caches
    // in the same frame as its ancestor: the scroll translation is applied
    // once, at the outermost cached boundary.
    if let Scene::Container(c) = scene
        && scene.is_cacheable_for_paint()
    {
        let hash = c.paint_hash();
        if session.fragment_cache.try_hit(hash, out, transform) {
            return;
        }
        // Cache miss: encode the entire subtree into a fresh sub-scene
        // under IDENTITY transform, then place + cache.
        //
        // R1527 — open the containment frame before descending so the
        // fragments encoded below attribute to this container. Closed by
        // `insert_miss` at the bottom of this arm, which every path from
        // here reaches.
        session.fragment_cache.enter_subtree();
        let mut sub = VelloScene::new();
        paint_box_decoration(&mut sub, c.rect, &c.style, c.style.fill, Affine::IDENTITY);
        for (index, child) in c.children.iter().enumerate() {
            to_vello_cached_inner(
                child,
                session,
                &mut sub,
                Affine::IDENTITY,
                placement,
                clip,
                Some(index),
            );
        }
        append_placed(out, &sub, transform);
        session
            .fragment_cache
            .insert_miss(hash, sub, c.rect, placement, clip);
        return;
    }

    // R706 §5.16 — "out receives appends only" invariant.
    //
    // Every node that is NOT served by the cacheable fast path above
    // encodes its own contribution into a FRESH sub-scene which is then
    // appended to `out`. This guarantees `out` only ever receives
    // `vello::Scene::append`s and never a direct `fill` / `stroke` /
    // `draw_glyphs` issued *after* an append.
    //
    // Why it matters: `vello_encoding::Encoding::append` copies the
    // appended child's force-transform/style flags (`self.flags =
    // other.flags`) and extends the transform stream, so the encoder's
    // "current transform" after an append is the child fragment's last
    // transform. A direct draw issued into the same scene afterwards
    // re-uses that stale transform-stream state and renders at the wrong
    // position. Before R706 the §5.39 keyboard focus-ring overlay
    // (a top-level `Scene::Box` injected as the last sibling of a root
    // whose earlier children were cached fragments) drew exactly one
    // grid column off because its `stroke` followed the cached grid's
    // `append`. Encoding each contribution into a clean sub and
    // appending sidesteps the hazard: within every sub the direct draws
    // come first and any child appends follow.
    let mut sub = VelloScene::new();
    match scene {
        Scene::Container(c) => {
            paint_box_decoration(&mut sub, c.rect, &c.style, c.style.fill, transform);
            for (index, child) in c.children.iter().enumerate() {
                to_vello_cached_inner(
                    child,
                    session,
                    &mut sub,
                    transform,
                    placement,
                    clip,
                    Some(index),
                );
            }
        }
        Scene::Box(b) => {
            let fill = (session.fill_hook)(b).unwrap_or(b.style.fill);
            paint_box_decoration(&mut sub, b.rect, &b.style, fill, transform);
        }
        Scene::Text(t) => paint_text(&mut sub, t, session.text_cache, session.engine, transform),
        Scene::Scroll(s) => {
            let viewport_clip = KurboRect::new(
                f64::from(s.viewport.x),
                f64::from(s.viewport.y),
                f64::from(s.viewport.x.saturating_add(s.viewport.w)),
                f64::from(s.viewport.y.saturating_add(s.viewport.h)),
            );
            sub.push_clip_layer(Fill::NonZero, transform, &viewport_clip);
            let dx = f64::from(s.viewport.x) - f64::from(s.offset_x);
            let dy = f64::from(s.viewport.y) - f64::from(s.offset_y);
            let shift = Affine::translate((dx, dy));
            // The viewport is expressed in the PARENT's frame, so `placement`
            // (the parent's screen transform) is what maps it to screen space,
            // and the result narrows whatever clip an outer scroll already
            // imposed. `transform` would be wrong here for the same reason it
            // is wrong for damage: inside a cached fragment it is IDENTITY.
            let viewport_clip_screen = screen_bounds(s.viewport, placement);
            let inner_clip = Some(match clip {
                Some(outer) => intersection(outer, viewport_clip_screen),
                None => viewport_clip_screen,
            });
            // R1557 §5.18 — `index: None`: a `Scroll` consumes no path segment
            // (`Scene::hit_test` / `collect_intersections`), so its content is
            // reached at the scroll's own address and the profile says so
            // rather than inventing a segment no other method would resolve.
            to_vello_cached_inner(
                &s.content,
                session,
                &mut sub,
                transform * shift,
                placement * shift,
                inner_clip,
                None,
            );
            sub.pop_layer();
        }
        Scene::ImmediateModeNode(node) => {
            paint_immediate_mode_node(&mut sub, node, transform);
        }
        Scene::Path(p) => paint_path(&mut sub, p, transform),
        // R740 §5.16 — raster image paint into the fresh sub-scene (the
        // R682 cache key already folds `ImageNode::source`, so a cached
        // fragment re-uses the decoded image and a source change misses).
        Scene::Image(i) => paint_image(&mut sub, i, session.image_cache, transform),
        // R991 §5.41 §2 #6 — TextGrid glyph paint into the fresh sub-scene
        // (uncacheable per `Scene::is_cacheable_for_paint`, so it is always
        // re-encoded; mirrors the External/ImmediateMode treatment).
        Scene::TextGrid(n) => {
            paint_text_grid(&mut sub, n, session.text_cache, transform, session.cursor);
        }
        // External / Effect: no-op (matches to_vello_inner's `_ => {}` arm).
        _ => {}
    }
    out.append(&sub, None);
}

/// R681 §2 #4 atomic 1 — paint one [`ImmediateModeNode`] into the
/// Vello scene. Wraps the inherited `transform` in a viewport-local
/// frame ([`Affine::translate`] over `(viewport.x, viewport.y)`),
/// constructs a [`VelloImmediatePainter`] over the resulting
/// transform, and invokes the driver's
/// [`pinion_core::scene::ImmediateMode::paint`] hook. The driver
/// rasterises in viewport-local coordinates; the painter applies
/// the composed transform at each Vello call site so the result
/// lands at the correct screen-space rect.
///
/// DPI scale is currently fixed at `1.0` (logical pixels). A
/// future round can thread the per-window DPI through the paint
/// adapter once a real consumer needs the hint.
fn paint_immediate_mode_node(
    out: &mut VelloScene,
    node: &ImmediateModeNode,
    parent_transform: Affine,
) {
    let viewport = node.viewport;
    if viewport.w == 0 || viewport.h == 0 {
        // Zero-size viewport: skip paint entirely (the driver may
        // emit `clear(...)` against a 0-size canvas, but a Vello
        // fill with a degenerate rect is a no-op anyway).
        return;
    }
    // Compose `parent_transform * translate(viewport.{x,y})` so
    // viewport-local `(0, 0)` lands at screen-space
    // `(viewport.x, viewport.y)` in the parent frame.
    let local_transform = node_local_transform(parent_transform, viewport);
    let mut painter = VelloImmediatePainter {
        out,
        viewport,
        transform: local_transform,
        dpi: 1.0,
    };
    node.handle.borrow_mut().paint(&mut painter);
}

/// R681 §2 #4 atomic 1 — Vello-backed [`ImmediatePainter`] impl.
/// Composes the inherited paint transform with a viewport-local
/// translation so the driver paints in viewport-local logical
/// pixels without knowing the parent transform chain (Scroll
/// content offset / Container nesting / etc.).
///
/// Lives in the paint-adapter module rather than a separate
/// `immediate.rs` because the Vello call shape (
/// [`vello::Scene::fill`] + [`vello::Scene::stroke`] +
/// [`vello::kurbo`] primitives) is the same surface the retained-tree
/// paint helpers reach for; sharing the module keeps the Vello
/// dependency localised.
pub struct VelloImmediatePainter<'a> {
    out: &'a mut VelloScene,
    viewport: Rect,
    transform: Affine,
    dpi: f32,
}

impl ImmediatePainter for VelloImmediatePainter<'_> {
    fn viewport_size(&self) -> (u32, u32) {
        (self.viewport.w, self.viewport.h)
    }

    fn dpi_scale(&self) -> f32 {
        self.dpi
    }

    fn clear(&mut self, color: Color) {
        if color == Color::TRANSPARENT {
            return;
        }
        let rect = KurboRect::new(
            0.0,
            0.0,
            f64::from(self.viewport.w),
            f64::from(self.viewport.h),
        );
        self.out
            .fill(Fill::NonZero, self.transform, to_peniko(color), None, &rect);
    }

    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        if color == Color::TRANSPARENT || w <= 0.0 || h <= 0.0 {
            return;
        }
        let rect = KurboRect::new(
            f64::from(x),
            f64::from(y),
            f64::from(x) + f64::from(w),
            f64::from(y) + f64::from(h),
        );
        self.out
            .fill(Fill::NonZero, self.transform, to_peniko(color), None, &rect);
    }

    fn fill_triangle(&mut self, p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), color: Color) {
        if color == Color::TRANSPARENT {
            return;
        }
        let path: BezPath = [
            PathEl::MoveTo(KurboPoint::new(f64::from(p1.0), f64::from(p1.1))),
            PathEl::LineTo(KurboPoint::new(f64::from(p2.0), f64::from(p2.1))),
            PathEl::LineTo(KurboPoint::new(f64::from(p3.0), f64::from(p3.1))),
            PathEl::ClosePath,
        ]
        .into_iter()
        .collect();
        self.out
            .fill(Fill::NonZero, self.transform, to_peniko(color), None, &path);
    }

    fn stroke_line(&mut self, p1: (f32, f32), p2: (f32, f32), width: f32, color: Color) {
        if color == Color::TRANSPARENT || width <= 0.0 {
            return;
        }
        let line = Line::new(
            KurboPoint::new(f64::from(p1.0), f64::from(p1.1)),
            KurboPoint::new(f64::from(p2.0), f64::from(p2.1)),
        );
        self.out.stroke(
            &Stroke::new(f64::from(width)),
            self.transform,
            to_peniko(color),
            None,
            &line,
        );
    }
}

/// Background color used as `RenderParams.base_color` — the surface
/// clear that happens *before* any scene draw. Resolves to the root
/// [`Scene::Container`]'s fill so a window resized larger than the
/// canonical scene rect stays visually consistent inside-vs-outside.
/// Any other root variant falls back to black (no canonical "scene
/// background" without a Container).
#[must_use]
pub fn root_background(scene: &Scene) -> PenikoColor {
    match scene {
        Scene::Container(c) => to_peniko(c.style.fill),
        _ => PenikoColor::BLACK,
    }
}

// R705 §5.39 §2 #1/#7 — `paint_focus_ring` + its `focus_rect_for_tag`
// walker were removed here. The focus ring is no longer an opaque vello
// stroke painted after the Scene→Vello tree walk (which was invisible to
// `scene/snapshot` and ignored the focused node's `corner_radius`). It is
// now injected upstream as a pointer-transparent, corner-radius-aware
// overlay `Scene::Box` by `pinion_overlay::inject_focus_ring`, applied as
// the final step of every paint-scene producer in `pinion-shell`, so the
// generic `to_vello` box path paints it and `scene/snapshot from: paint`
// observes it. See `pinion_overlay::focus_ring`.

/// Convert a pinion [`Color`] to a peniko `Color`, preserving every
/// channel including alpha. The §5.3 R20 `Color::rgba(r, g, b, a)`
/// shape is the source of truth; the legacy [`Color::from_argb`]
/// decoder reads the high `0xAA__` byte verbatim too, so callers that
/// want explicit opacity must construct via [`Color::rgb`] /
/// [`Color::rgba`] rather than the softbuffer-style `0x00RRGGBB`
/// literal (which decodes to alpha = 0 = fully transparent on Vello).
#[must_use]
pub fn to_peniko(c: Color) -> PenikoColor {
    PenikoColor::from_rgba8(c.r, c.g, c.b, c.a)
}

// ---------------------------------------------------------------------------
// R1063 §5.37 → §5.16 — self-hosted text engine → production-paint BRING-UP seam.
//
// The §5.37 self-hosted engine (`pinion-text-font`) shapes + rasterises text
// with zero external deps but had no production pixel consumer — every layer
// was a test forcing-consumer. This is the first wire reaching real pixels:
// `render_paragraph` / `render_lines`' composited [`Coverage`] AA mask is
// uploaded as a `peniko::ImageData` and blitted through Vello's existing
// image-texture path ([`vello::Scene::draw_image`], the same pipeline
// `Scene::Image` uses), rather than hand-rolling a wgpu texture upload.
//
// IMPORTANT — this is a BRING-UP seam, NOT the production text primitive. It
// blits the *whole-paragraph composited* mask (one texture per paragraph,
// re-uploaded each frame it changes). The production GPU-text primitive is the
// per-glyph [`GlyphAtlas`](pinion_text_font::GlyphAtlas) (§5.37.9), which
// `render_glyphs` builds and `composite` then flattens away before this seam
// sees it — a cacheable atlas sampled per glyph-quad is what a 60fps / AAA text
// path needs. The next wiring round must target the atlas; this whole-paragraph
// blit must NOT be generalised into the `Scene::Text` arm.
//
// So this round delivers a *seam* + forcing consumers only. The §5.37 →
// production migration is a multi-round campaign — atlas-based paint, then
// caret / selection / metrics / fallback parity against the §5.36 parley/swash
// bridge, then multi-font atlas binding — not a single `Scene::Text` rewire.
// ---------------------------------------------------------------------------

/// R1065 §5.37 → §5.16 — pack a grayscale AA coverage buffer into a straight-alpha
/// RGBA8 [`ImageData`] tinted by `color`.
///
/// `alpha` is one mask byte per pixel, row-major `width × height`. The mask
/// supplies per-pixel **alpha**; `color` supplies the constant RGB (and its own
/// alpha modulates the mask, so a translucent brush dims the whole run). Pixels
/// are straight (un-premultiplied) RGBA8 with [`ImageAlphaType::Alpha`], matching
/// the §5.16 [`ImageCache`] image path so Vello composites with `src-over`
/// antialiasing exactly like a raster image leaf.
///
/// The SSOT pixel conversion (R1065 lift) shared by the whole-paragraph
/// [`coverage_to_image_data`] (R1063) and the per-glyph-atlas
/// [`atlas_to_image_data`] (R1065). Returns `None` for an empty buffer (a space /
/// control glyph, or an atlas with nothing packed — nothing to paint), or when
/// the dimensions do not fit a `u32` (defensive; the rasterizer caps each axis
/// well under that).
fn mask_to_image_data(
    alpha: &[u8],
    width: usize,
    height: usize,
    color: Color,
) -> Option<ImageData> {
    if alpha.is_empty() {
        return None;
    }
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return None;
    };
    let mut rgba = Vec::with_capacity(alpha.len() * 4);
    for &mask in alpha {
        // Modulate the mask coverage by the brush colour's own alpha, rounding
        // to nearest (`+ 127`) to match the §5.37 pipeline's house convention
        // for this `a·b / 255` operation — the atlas compositor `shape::over`
        // and the rasterizer's coverage quantization both round, so flooring
        // here would bias every antialiased run ≤1 LSB darker than the engine.
        // The product is bounded by 255·255 + 127 = 65152 < u16::MAX.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "(u16 product + 127) / 255 is in 0..=255, fits u8 exactly"
        )]
        let out_a = ((u16::from(mask) * u16::from(color.a) + 127) / 255) as u8;
        rgba.extend_from_slice(&[color.r, color.g, color.b, out_a]);
    }
    let pixels: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(rgba);
    Some(ImageData {
        data: Blob::new(pixels),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    })
}

/// R1063 §5.37 → §5.16 — convert a self-hosted [`Coverage`] AA mask into a
/// `peniko::ImageData` ready for [`vello::Scene::draw_image`]. A thin wrapper
/// over the SSOT `mask_to_image_data`: the mask supplies per-pixel alpha,
/// `color` the constant RGB (its own alpha modulating the mask). Returns `None`
/// for an empty mask.
#[must_use]
pub fn coverage_to_image_data(coverage: &Coverage, color: Color) -> Option<ImageData> {
    mask_to_image_data(&coverage.alpha, coverage.width, coverage.height, color)
}

/// R1065 §5.37 → §5.16 — convert a whole [`GlyphAtlas`] bitmap into one tinted
/// `peniko::ImageData`, uploaded once and sampled per glyph-quad by
/// [`draw_atlased_glyphs`]. A thin wrapper over the SSOT `mask_to_image_data`.
/// Returns `None` for an atlas with no packed pixels (no glyph rasterized yet).
#[must_use]
pub fn atlas_to_image_data(atlas: &GlyphAtlas, color: Color) -> Option<ImageData> {
    mask_to_image_data(atlas.alpha(), atlas.width(), atlas.height(), color)
}

/// R1063 §5.37 → §5.16 — blit a self-hosted [`Coverage`] mask into the Vello
/// scene at baseline pen `(pen_x, pen_y)` (device px, y-down) in `color`,
/// under `transform`.
///
/// The mask's `left` / `top` offset positions the bitmap relative to the pen
/// (per [`Coverage`]: the top-left pixel lands at `(pen_x + left,
/// pen_y + top)`). The image is drawn 1:1 (no scaling — it is already
/// rasterised at the target px/em). A no-op for an empty mask.
///
/// Retained as the single-image alternative to the per-glyph
/// [`draw_atlased_glyphs`]: one whole-paragraph upload, cheaper for a
/// uniform-colour run, and the bring-up contrast witness for the atlas path.
pub fn draw_coverage(
    out: &mut VelloScene,
    coverage: &Coverage,
    color: Color,
    pen_x: f64,
    pen_y: f64,
    transform: Affine,
) {
    let Some(image) = coverage_to_image_data(coverage, color) else {
        return;
    };
    let place = transform
        * Affine::translate((
            pen_x + f64::from(coverage.left),
            pen_y + f64::from(coverage.top),
        ));
    // Nearest sampling for seam-wide consistency with the atlas path; for this
    // 1:1 integer blit it is crisp and equivalent to the default bilinear.
    out.draw_image(
        &ImageBrush::new(image).with_quality(ImageQuality::Low),
        place,
    );
}

/// R1065 §5.37 → §5.16 — paint a [`RenderedGlyphs`] per glyph in one uniform
/// `color`, at baseline pen `(pen_x, pen_y)` (device px, y-down) under
/// `transform`. A thin wrapper over [`draw_atlased_glyphs_styled`] with `color`
/// applied to every glyph.
///
/// The §5.37.9 production-direction successor to [`draw_coverage`]: where
/// `draw_coverage` blits one whole-paragraph mask, this keeps the cacheable
/// per-glyph atlas — each glyph is a [`vello::Scene::fill`] of its device quad
/// with the atlas image brush, `brush_transform` aligning the atlas sub-rect
/// under the quad, and **nearest** sampling ([`ImageQuality::Low`]). With an
/// integer-aligned device quad (integer pen + integer-translation `transform`,
/// as every current caller passes) no glyph bleeds an adjacent shelf-packed atlas
/// glyph — each covered pixel samples its own glyph's texel. A *fractional*
/// placement (`HiDPI` fractional scale, sub-pixel scroll) can sample a neighbour
/// at the quad edge: the atlas has no inter-glyph gutter yet, so that hardening is
/// deferred to the production `HiDPI` consumer (no integer-pen caller bleeds today).
/// A no-op for no placements.
pub fn draw_atlased_glyphs(
    out: &mut VelloScene,
    rendered: &RenderedGlyphs,
    color: Color,
    pen_x: f64,
    pen_y: f64,
    transform: Affine,
) {
    let colors = vec![color; rendered.placed.len()];
    draw_atlased_glyphs_styled(out, rendered, &colors, pen_x, pen_y, transform);
}

/// R1066 §5.37 → §5.16 — paint a [`RenderedGlyphs`] per glyph in **per-glyph
/// colours** (`glyph_colors[i]` tints `rendered.placed[i]`) — the styled-run
/// paint a code editor needs (syntax highlighting): one atlas, glyphs in many
/// colours.
///
/// The atlas itself is grayscale (glyph *shape* is colour-independent); colour is
/// applied at paint by tinting. So one tinted image is uploaded per distinct
/// `(atlas, colour)` actually drawn — a K-colour run uploads K tints of an atlas,
/// **not** N per-glyph images (the costly coverage rasterisation stays once-per
/// glyph in the atlas; only the cheap tint is per colour). Each glyph is then the
/// same per-quad `fill` as the uniform [`draw_atlased_glyphs`] (nearest-sampled;
/// the no-bleed guarantee is conditional on an integer-aligned quad — see there).
///
/// `glyph_colors` aligns to `rendered.placed`; a glyph past the end of
/// `glyph_colors` is skipped (defensive — the uniform wrapper always supplies one
/// colour per glyph). A no-op for no placements.
pub fn draw_atlased_glyphs_styled(
    out: &mut VelloScene,
    rendered: &RenderedGlyphs,
    glyph_colors: &[Color],
    pen_x: f64,
    pen_y: f64,
    transform: Affine,
) {
    // Pass 1: build one tinted brush per distinct (atlas, colour) actually drawn.
    // `None` = that atlas has nothing packed (never referenced — blank glyphs are
    // not placed).
    let mut brushes: HashMap<(usize, Color), Option<ImageBrush>> = HashMap::new();
    for (i, p) in rendered.placed.iter().enumerate() {
        let Some(&color) = glyph_colors.get(i) else {
            continue; // fewer colours than glyphs — drop the uncoloured tail.
        };
        brushes.entry((p.atlas, color)).or_insert_with(|| {
            rendered.atlases.get(p.atlas).and_then(|atlas| {
                atlas_to_image_data(atlas, color)
                    .map(|img| ImageBrush::new(img).with_quality(ImageQuality::Low))
            })
        });
    }

    // Pass 2: fill each glyph's quad from its (atlas, colour) brush.
    for (i, p) in rendered.placed.iter().enumerate() {
        let Some(&color) = glyph_colors.get(i) else {
            continue;
        };
        let Some(Some(brush)) = brushes.get(&(p.atlas, color)) else {
            continue; // defensive: empty / out-of-range atlas.
        };
        let g = &p.glyph;
        // Device-space top-left of the glyph quad: baseline pen + the glyph's
        // pen origin + its rasterized-bitmap offset (`left`/`top`).
        let dst_x = pen_x + f64::from(p.pen_x) + f64::from(g.left);
        let dst_y = pen_y + f64::from(p.pen_y) + f64::from(g.top);
        #[allow(
            clippy::cast_precision_loss,
            reason = "atlas pixel coords/dims are well under 2^53 — exact in f64"
        )]
        let (gw, gh, gx, gy) = (g.width as f64, g.height as f64, g.x as f64, g.y as f64);
        let quad = KurboRect::new(dst_x, dst_y, dst_x + gw, dst_y + gh);
        // Map atlas-pixel space → user space: image pixel `(g.x + u, g.y + v)`
        // must land at `(dst_x + u, dst_y + v)` — a pure translation (the glyph is
        // rasterised at the target px/em, so no scaling).
        let brush_transform = Affine::translate((dst_x - gx, dst_y - gy));
        out.fill(
            Fill::NonZero,
            transform,
            brush,
            Some(brush_transform),
            &quad,
        );
    }
}

/// R991.1 §5.16 R1531 §5.36 — emit one [`PositionedRun`]'s glyphs into the
/// Vello scene at `transform` in `brush`. The shared glyph emit behind
/// [`paint_text`] (per-run styled brush + decorations) and [`paint_text_grid`]
/// (per-cell solid brush). Decorations stay in the caller because the two
/// derive them differently: [`paint_text`] strokes the run's own font-metric
/// underline / strikethrough spanning its advance, while [`paint_text_grid`]
/// paints SGR rules spanning the full cell at cell-geometry offsets — both
/// through the shared [`stroke_hrule`] primitive.
///
/// R1531 — the run arrives already positioned. This used to take a parley
/// `GlyphRun` and call `positioned_glyphs()`, re-deriving the pen positions on
/// every paint of unchanged text; the derivation now happens once per shaped
/// layout inside the cache that holds it. Mapping to `vello::Glyph` is a field
/// copy over a slice, which is what makes the renderer-agnostic
/// [`PositionedGlyph`](pinion_text::PositionedGlyph) free at this boundary.
fn draw_positioned_run(
    out: &mut VelloScene,
    run: &PositionedRun,
    transform: Affine,
    brush: PenikoColor,
) {
    out.draw_glyphs(&run.font)
        .transform(transform)
        .font_size(run.font_size)
        .brush(brush)
        .draw(
            Fill::NonZero,
            run.glyphs.iter().map(|g| Glyph {
                id: g.id,
                x: g.x,
                y: g.y,
            }),
        );
}

/// R992 §5.41 §5.16 — stroke one horizontal rule from (`x0`, `y`) to
/// (`x1`, `y`) with pen width `max(width, 1)` in `brush`, under `transform`.
/// The shared primitive behind styled-text decorations ([`paint_decorations`]:
/// the font-metric underline / strikethrough spanning a glyph-run advance) and
/// the cell grid's SGR underline / strikethrough ([`paint_text_grid`]: spanning
/// the full cell width). A zero-or-sub-pixel `width` clamps to a 1.0-px hairline
/// so a thin font metric never produces an invisible rule.
fn stroke_hrule(
    out: &mut VelloScene,
    transform: Affine,
    brush: PenikoColor,
    x0: f64,
    x1: f64,
    y: f64,
    width: f64,
) {
    let line = Line::new((x0, y), (x1, y));
    out.stroke(&Stroke::new(width.max(1.0)), transform, brush, None, &line);
}

/// R1399 §5.41 — paint one cell's underline in the given `brush` for its
/// SGR 4:x [`UnderlineStyle`]. `[x0, x1)` is the cell's horizontal span and
/// `cell_bottom` its bottom edge; the rule (or wave) sits just inside the
/// bottom in a `rule_w`-thick stroke. The caller has already resolved the
/// underline colour (explicit SGR-58 or the effective foreground) and only
/// calls this for a drawn style ([`UnderlineStyle::is_on`]).
///
/// The dotted / dashed forms are drawn as explicit short segments rather
/// than a `kurbo` dash pattern: Vello's sparse rasteriser handles a plain
/// stroked `Line` / `BezPath` predictably, and short segments keep the
/// `headless_screenshot` pixel witness deterministic — the same reason the
/// overlay edges avoid exotic stroke features.
#[allow(clippy::too_many_arguments)]
fn paint_underline(
    out: &mut VelloScene,
    transform: Affine,
    brush: PenikoColor,
    x0: f64,
    x1: f64,
    cell_bottom: f64,
    rule_w: f64,
    style: UnderlineStyle,
) {
    // The lower straight rule sits one stroke-width above the cell bottom.
    let y_low = cell_bottom - rule_w;
    match style {
        // Handled by the caller's `is_on` guard, but keep the match total.
        UnderlineStyle::None => {}
        UnderlineStyle::Single => stroke_hrule(out, transform, brush, x0, x1, y_low, rule_w),
        UnderlineStyle::Double => {
            // A second rule a clear gap (2·rule_w) above the first.
            stroke_hrule(out, transform, brush, x0, x1, y_low, rule_w);
            stroke_hrule(out, transform, brush, x0, x1, y_low - 2.0 * rule_w, rule_w);
        }
        UnderlineStyle::Curly => {
            // An undercurl: a smooth sinusoid of quadratic arcs about a
            // centre line, amplitude `rule_w` so the wave stays within the
            // cell's bottom band. Half-wavelength `hp` ties the squiggle
            // density to the stroke weight.
            let amp = rule_w;
            let mid = cell_bottom - rule_w - amp;
            let hp = (amp * 2.0).max(2.0);
            let mut path = BezPath::new();
            path.move_to(KurboPoint::new(x0, mid));
            let mut x = x0;
            let mut up = true;
            while x < x1 - f64::EPSILON {
                let nx = (x + hp).min(x1);
                let ctrl_x = x + (nx - x) * 0.5;
                let ctrl_y = if up { mid - amp } else { mid + amp };
                path.quad_to(KurboPoint::new(ctrl_x, ctrl_y), KurboPoint::new(nx, mid));
                x = nx;
                up = !up;
            }
            out.stroke(&Stroke::new(rule_w.max(1.0)), transform, brush, None, &path);
        }
        UnderlineStyle::Dotted => {
            // Short dots: a `dash` on, a `2·dash` gap off — sparser than the
            // dashed form so the two read (and measure) distinctly.
            let dash = rule_w.max(1.0);
            stroke_dashed_hrule(
                out,
                transform,
                brush,
                x0,
                x1,
                y_low,
                rule_w,
                dash,
                dash * 2.0,
            );
        }
        UnderlineStyle::Dashed => {
            // Longer dashes: a quarter-cell on, an eighth-cell off.
            let span = x1 - x0;
            let dash = (span / 4.0).max(rule_w * 3.0);
            let gap = (span / 8.0).max(rule_w * 2.0);
            stroke_dashed_hrule(out, transform, brush, x0, x1, y_low, rule_w, dash, gap);
        }
    }
}

/// R1399 §5.41 — a horizontal rule at `y` broken into `dash`-long segments
/// separated by `gap`-long holes, in `rule_w`-thick strokes. Used for the
/// dotted / dashed underline forms (see [`paint_underline`]). Segments are
/// drawn explicitly (not via a `kurbo` dash pattern) for a deterministic
/// pixel witness.
#[allow(clippy::too_many_arguments)]
fn stroke_dashed_hrule(
    out: &mut VelloScene,
    transform: Affine,
    brush: PenikoColor,
    x0: f64,
    x1: f64,
    y: f64,
    rule_w: f64,
    dash: f64,
    gap: f64,
) {
    let step = dash + gap;
    if step <= 0.0 {
        return;
    }
    let mut x = x0;
    while x < x1 - f64::EPSILON {
        let seg_end = (x + dash).min(x1);
        stroke_hrule(out, transform, brush, x, seg_end, y, rule_w);
        x += step;
    }
}

/// R993 §5.41 — shape one cell's grapheme `cell.cluster` (with its SGR
/// bold / italic weight / slant) through the shared `cache` and draw the
/// glyph run at `glyph_transform` in `brush`. The shared cell-glyph emit
/// behind the main grid pass ([`paint_text_grid`]) and the block-cursor
/// inverse redraw — R991's colour-independent cache holds (the brush is
/// applied at draw time, so one `Layout` per cluster serves any colour, and
/// bold / italic are distinct keys). The caller guarantees the cluster is
/// non-empty and not a [`CellWidth::Trailer`].
fn draw_cell_glyph(
    out: &mut VelloScene,
    cache: &mut LayoutCache,
    base_style: &TextStyle,
    cell: &TermCell,
    glyph_transform: Affine,
    brush: PenikoColor,
) {
    let runs = if cell.attrs.bold || cell.attrs.italic {
        let mut styled = base_style.clone();
        if cell.attrs.bold {
            styled.font_weight = FontWeight::BOLD;
        }
        if cell.attrs.italic {
            styled.font_style = FontStyle::Italic;
        }
        cache.positioned_runs(&cell.cluster, &styled, &[], None)
    } else {
        cache.positioned_runs(&cell.cluster, base_style, &[], None)
    };
    // The cell's ink colour comes from the terminal palette (SGR + reverse +
    // dim, resolved by the caller), not from the shaped style, so the run's
    // own `brush` is ignored here — see [`PositionedRun::brush`].
    for run in runs {
        draw_positioned_run(out, run, glyph_transform, brush);
    }
}

/// R994.1 §5.41 — the cell's effective `(fg, bg)` terms after SGR 7 reverse,
/// the swap applied *before* palette resolution. The renderer owns the swap
/// (a `scene/snapshot` reports the *stored* flag + colours, leaving it to "a
/// renderer" per the [`CellAttrs`](pinion_core::term_grid::CellAttrs)
/// contract), so this is a Vello-local SSOT shared by the cell pass and the
/// cursor overlay — not a core method.
fn effective_terms(cell: &TermCell) -> (TermColor, TermColor) {
    if cell.attrs.reverse {
        (cell.bg, cell.fg)
    } else {
        (cell.fg, cell.bg)
    }
}

/// R994.1 §5.41 — whether a cell carries a drawable glyph cluster: a
/// [`CellWidth::Trailer`] has none (the wide head draws it) and an
/// all-whitespace cluster inks nothing. The trailer + blank SSOT shared by
/// the main glyph pass and the block-cursor inverse redraw; SGR 8 `hidden`
/// is *not* folded in because the two callers suppress it differently (the
/// main pass `continue`s early to also drop the decorations, the cursor only
/// skips the glyph redraw).
fn has_glyph_cluster(cell: &TermCell) -> bool {
    cell.width != CellWidth::Trailer && !cell.cluster.trim().is_empty()
}

/// R1178 §5.41 — geometric synthesis of the **solid** Unicode Block Elements
/// (`U+2580`–`U+2590`, `U+2594`–`U+259F`). A solid block tiles its cell by an
/// exact fraction — full / halves / eighths / quadrants — so painting it as a
/// font glyph leaves the fitted-size + bearing margin that R1002's
/// `*_loose_for_mismatched_cell` pins, and block art (terminal logos, progress
/// bars, quadrant mosaics) shows inter-cell gaps. Returning the cell-exact fill
/// rectangles lets [`paint_text_grid`] fill them in the foreground brush
/// instead, so adjacent cells abut with no gap.
///
/// Returns `Some((rects, count))` — at most four sub-cell rectangles in
/// absolute device pixels — for a solid block codepoint `c`, or `None` for
/// anything else: the shade blocks `U+2591`–`U+2593` (an alpha pattern, not a
/// solid fill) and the box-drawing range `U+2500`–`U+257F` both return `None`
/// so the caller falls through to shade / box / glyph. The lone-codepoint /
/// range gate is the caller's ([`synthesizable_char`]).
///
/// Split points are snapped to integer pixels so a quadrant combo's interior
/// edges (and a full block's cell edges) land on exact pixel boundaries: two
/// abutting fills then share a crisp edge with no anti-aliasing seam, and a row
/// of full blocks tiles seamlessly (the R1178 acceptance criterion).
fn block_element_rects(c: char, cell: KurboRect) -> Option<([KurboRect; 4], usize)> {
    let (cx, cy, cell_w, cell_h) = (cell.x0, cell.y0, cell.width(), cell.height());
    let x0 = cx;
    let y0 = cy;
    let x1 = cx + cell_w;
    let y1 = cy + cell_h;
    // Integer-snapped half / eighth split points (so abutting fills share an
    // exact pixel boundary instead of a sub-pixel AA seam).
    let xm = cx + (cell_w / 2.0).round();
    let ym = cy + (cell_h / 2.0).round();
    let frac_x = |k: f64| cx + (k * cell_w / 8.0).round();
    let frac_y = |k: f64| cy + (k * cell_h / 8.0).round();
    let solo = |r: KurboRect| Some(([r, KurboRect::ZERO, KurboRect::ZERO, KurboRect::ZERO], 1));
    match c {
        // Full block — the whole cell (exact integer edges => seamless tiling).
        '\u{2588}' => solo(KurboRect::new(x0, y0, x1, y1)),
        // Upper blocks (top fraction of the cell).
        '\u{2580}' => solo(KurboRect::new(x0, y0, x1, ym)), // upper half
        '\u{2594}' => solo(KurboRect::new(x0, y0, x1, frac_y(1.0))), // upper 1/8
        // Lower blocks (bottom k/8 of the cell): U+2581..=U+2587.
        '\u{2581}' => solo(KurboRect::new(x0, frac_y(7.0), x1, y1)),
        '\u{2582}' => solo(KurboRect::new(x0, frac_y(6.0), x1, y1)),
        '\u{2583}' => solo(KurboRect::new(x0, frac_y(5.0), x1, y1)),
        '\u{2584}' => solo(KurboRect::new(x0, ym, x1, y1)), // lower half (4/8)
        '\u{2585}' => solo(KurboRect::new(x0, frac_y(3.0), x1, y1)),
        '\u{2586}' => solo(KurboRect::new(x0, frac_y(2.0), x1, y1)),
        '\u{2587}' => solo(KurboRect::new(x0, frac_y(1.0), x1, y1)),
        // Left blocks (left k/8 of the cell): U+2589..=U+258F.
        '\u{2589}' => solo(KurboRect::new(x0, y0, frac_x(7.0), y1)),
        '\u{258A}' => solo(KurboRect::new(x0, y0, frac_x(6.0), y1)),
        '\u{258B}' => solo(KurboRect::new(x0, y0, frac_x(5.0), y1)),
        '\u{258C}' => solo(KurboRect::new(x0, y0, xm, y1)), // left half (4/8)
        '\u{258D}' => solo(KurboRect::new(x0, y0, frac_x(3.0), y1)),
        '\u{258E}' => solo(KurboRect::new(x0, y0, frac_x(2.0), y1)),
        '\u{258F}' => solo(KurboRect::new(x0, y0, frac_x(1.0), y1)),
        // Right blocks.
        '\u{2590}' => solo(KurboRect::new(xm, y0, x1, y1)), // right half
        '\u{2595}' => solo(KurboRect::new(frac_x(7.0), y0, x1, y1)), // right 1/8
        // Quadrants — the filled subset of {UL, UR, LL, LR} as a bitmask:
        // bit0 = UL, bit1 = UR, bit2 = LL, bit3 = LR.
        _ => {
            let mask: u8 = match c {
                '\u{2596}' => 0b0100, // LL
                '\u{2597}' => 0b1000, // LR
                '\u{2598}' => 0b0001, // UL
                '\u{2599}' => 0b1101, // UL + LL + LR
                '\u{259A}' => 0b1001, // UL + LR
                '\u{259B}' => 0b0111, // UL + UR + LL
                '\u{259C}' => 0b1011, // UL + UR + LR
                '\u{259D}' => 0b0010, // UR
                '\u{259E}' => 0b0110, // UR + LL
                '\u{259F}' => 0b1110, // UR + LL + LR
                _ => return None,
            };
            let quads = [
                KurboRect::new(x0, y0, xm, ym), // UL
                KurboRect::new(xm, y0, x1, ym), // UR
                KurboRect::new(x0, ym, xm, y1), // LL
                KurboRect::new(xm, ym, x1, y1), // LR
            ];
            let mut rects = [KurboRect::ZERO; 4];
            let mut n = 0;
            for (i, quad) in quads.iter().enumerate() {
                if mask & (1u8 << i) != 0 {
                    rects[n] = *quad;
                    n += 1;
                }
            }
            Some((rects, n))
        }
    }
}

/// R1179 §5.41 — the Unicode **shade** blocks (`U+2591` LIGHT / `U+2592` MEDIUM
/// / `U+2593` DARK SHADE) as the ink-alpha fraction `(num, den)`. Unlike the
/// solid blocks these are not a coverage shape but a stipple: every serious
/// terminal renders them as the foreground blended over the cell background at
/// 25 / 50 / 75 %, which an alpha fill reproduces exactly (no sub-pixel pattern
/// needed). `None` for anything else (the caller falls through to solid-block,
/// box-drawing, then the glyph path).
fn shade_block_fraction(c: char) -> Option<(u16, u16)> {
    match c {
        '\u{2591}' => Some((1, 4)), // LIGHT SHADE  — 25%
        '\u{2592}' => Some((1, 2)), // MEDIUM SHADE — 50%
        '\u{2593}' => Some((3, 4)), // DARK SHADE   — 75%
        _ => None,
    }
}

/// R1180 §5.41 — a box-drawing glyph (`U+2500`–`U+257F`) decomposed for
/// geometric synthesis. The same cell-tiling rationale as the block elements:
/// a box-drawing line is meant to abut its neighbours into a continuous rule,
/// which a font glyph's advance / bearing cannot guarantee.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BoxGlyph {
    /// Straight lines / junctions / doubles / dashes built from four arms.
    /// Each arm weight: `0` none, `1` light, `2` heavy, `3` double. `dash` is
    /// `0` for solid, else the dash count (`2`/`3`/`4`) of the single-axis
    /// dashed glyphs.
    Arms {
        up: u8,
        down: u8,
        left: u8,
        right: u8,
        dash: u8,
    },
    /// A light rounded corner: a vertical arm (`down` else up) and a horizontal
    /// arm (`right` else left) joined by a quarter curve through the centre.
    Arc { down: bool, right: bool },
    /// Light diagonal stroke(s): forward slash and/or back slash.
    Diagonal { slash: bool, backslash: bool },
}

/// R1180 §5.41 — classify a char in the box-drawing block (`U+2500`–`U+257F`)
/// into a [`BoxGlyph`], or `None` for anything outside it. The arm-weight table
/// is the canonical Unicode decomposition (up / down / left / right ∈ none /
/// light / heavy / double, plus dashed and the rounded / diagonal specials).
#[allow(clippy::too_many_lines)] // a 128-codepoint Unicode lookup table
fn box_drawing(c: char) -> Option<BoxGlyph> {
    use BoxGlyph::{Arc, Arms, Diagonal};
    let a = |up: u8, down: u8, left: u8, right: u8, dash: u8| Arms {
        up,
        down,
        left,
        right,
        dash,
    };
    Some(match c {
        '\u{2500}' => a(0, 0, 1, 1, 0), // ─
        '\u{2501}' => a(0, 0, 2, 2, 0), // ━
        '\u{2502}' => a(1, 1, 0, 0, 0), // │
        '\u{2503}' => a(2, 2, 0, 0, 0), // ┃
        '\u{2504}' => a(0, 0, 1, 1, 3), // ┄
        '\u{2505}' => a(0, 0, 2, 2, 3), // ┅
        '\u{2506}' => a(1, 1, 0, 0, 3), // ┆
        '\u{2507}' => a(2, 2, 0, 0, 3), // ┇
        '\u{2508}' => a(0, 0, 1, 1, 4), // ┈
        '\u{2509}' => a(0, 0, 2, 2, 4), // ┉
        '\u{250A}' => a(1, 1, 0, 0, 4), // ┊
        '\u{250B}' => a(2, 2, 0, 0, 4), // ┋
        '\u{250C}' => a(0, 1, 0, 1, 0), // ┌
        '\u{250D}' => a(0, 1, 0, 2, 0), // ┍
        '\u{250E}' => a(0, 2, 0, 1, 0), // ┎
        '\u{250F}' => a(0, 2, 0, 2, 0), // ┏
        '\u{2510}' => a(0, 1, 1, 0, 0), // ┐
        '\u{2511}' => a(0, 1, 2, 0, 0), // ┑
        '\u{2512}' => a(0, 2, 1, 0, 0), // ┒
        '\u{2513}' => a(0, 2, 2, 0, 0), // ┓
        '\u{2514}' => a(1, 0, 0, 1, 0), // └
        '\u{2515}' => a(1, 0, 0, 2, 0), // ┕
        '\u{2516}' => a(2, 0, 0, 1, 0), // ┖
        '\u{2517}' => a(2, 0, 0, 2, 0), // ┗
        '\u{2518}' => a(1, 0, 1, 0, 0), // ┘
        '\u{2519}' => a(1, 0, 2, 0, 0), // ┙
        '\u{251A}' => a(2, 0, 1, 0, 0), // ┚
        '\u{251B}' => a(2, 0, 2, 0, 0), // ┛
        '\u{251C}' => a(1, 1, 0, 1, 0), // ├
        '\u{251D}' => a(1, 1, 0, 2, 0), // ┝
        '\u{251E}' => a(2, 1, 0, 1, 0), // ┞
        '\u{251F}' => a(1, 2, 0, 1, 0), // ┟
        '\u{2520}' => a(2, 2, 0, 1, 0), // ┠
        '\u{2521}' => a(2, 1, 0, 2, 0), // ┡
        '\u{2522}' => a(1, 2, 0, 2, 0), // ┢
        '\u{2523}' => a(2, 2, 0, 2, 0), // ┣
        '\u{2524}' => a(1, 1, 1, 0, 0), // ┤
        '\u{2525}' => a(1, 1, 2, 0, 0), // ┥
        '\u{2526}' => a(2, 1, 1, 0, 0), // ┦
        '\u{2527}' => a(1, 2, 1, 0, 0), // ┧
        '\u{2528}' => a(2, 2, 1, 0, 0), // ┨
        '\u{2529}' => a(2, 1, 2, 0, 0), // ┩
        '\u{252A}' => a(1, 2, 2, 0, 0), // ┪
        '\u{252B}' => a(2, 2, 2, 0, 0), // ┫
        '\u{252C}' => a(0, 1, 1, 1, 0), // ┬
        '\u{252D}' => a(0, 1, 2, 1, 0), // ┭
        '\u{252E}' => a(0, 1, 1, 2, 0), // ┮
        '\u{252F}' => a(0, 1, 2, 2, 0), // ┯
        '\u{2530}' => a(0, 2, 1, 1, 0), // ┰
        '\u{2531}' => a(0, 2, 2, 1, 0), // ┱
        '\u{2532}' => a(0, 2, 1, 2, 0), // ┲
        '\u{2533}' => a(0, 2, 2, 2, 0), // ┳
        '\u{2534}' => a(1, 0, 1, 1, 0), // ┴
        '\u{2535}' => a(1, 0, 2, 1, 0), // ┵
        '\u{2536}' => a(1, 0, 1, 2, 0), // ┶
        '\u{2537}' => a(1, 0, 2, 2, 0), // ┷
        '\u{2538}' => a(2, 0, 1, 1, 0), // ┸
        '\u{2539}' => a(2, 0, 2, 1, 0), // ┹
        '\u{253A}' => a(2, 0, 1, 2, 0), // ┺
        '\u{253B}' => a(2, 0, 2, 2, 0), // ┻
        '\u{253C}' => a(1, 1, 1, 1, 0), // ┼
        '\u{253D}' => a(1, 1, 2, 1, 0), // ┽
        '\u{253E}' => a(1, 1, 1, 2, 0), // ┾
        '\u{253F}' => a(1, 1, 2, 2, 0), // ┿
        '\u{2540}' => a(2, 1, 1, 1, 0), // ╀
        '\u{2541}' => a(1, 2, 1, 1, 0), // ╁
        '\u{2542}' => a(2, 2, 1, 1, 0), // ╂
        '\u{2543}' => a(2, 1, 2, 1, 0), // ╃
        '\u{2544}' => a(2, 1, 1, 2, 0), // ╄
        '\u{2545}' => a(1, 2, 2, 1, 0), // ╅
        '\u{2546}' => a(1, 2, 1, 2, 0), // ╆
        '\u{2547}' => a(2, 1, 2, 2, 0), // ╇
        '\u{2548}' => a(1, 2, 2, 2, 0), // ╈
        '\u{2549}' => a(2, 2, 2, 1, 0), // ╉
        '\u{254A}' => a(2, 2, 1, 2, 0), // ╊
        '\u{254B}' => a(2, 2, 2, 2, 0), // ╋
        '\u{254C}' => a(0, 0, 1, 1, 2), // ╌
        '\u{254D}' => a(0, 0, 2, 2, 2), // ╍
        '\u{254E}' => a(1, 1, 0, 0, 2), // ╎
        '\u{254F}' => a(2, 2, 0, 0, 2), // ╏
        '\u{2550}' => a(0, 0, 3, 3, 0), // ═
        '\u{2551}' => a(3, 3, 0, 0, 0), // ║
        '\u{2552}' => a(0, 1, 0, 3, 0), // ╒
        '\u{2553}' => a(0, 3, 0, 1, 0), // ╓
        '\u{2554}' => a(0, 3, 0, 3, 0), // ╔
        '\u{2555}' => a(0, 1, 3, 0, 0), // ╕
        '\u{2556}' => a(0, 3, 1, 0, 0), // ╖
        '\u{2557}' => a(0, 3, 3, 0, 0), // ╗
        '\u{2558}' => a(1, 0, 0, 3, 0), // ╘
        '\u{2559}' => a(3, 0, 0, 1, 0), // ╙
        '\u{255A}' => a(3, 0, 0, 3, 0), // ╚
        '\u{255B}' => a(1, 0, 3, 0, 0), // ╛
        '\u{255C}' => a(3, 0, 1, 0, 0), // ╜
        '\u{255D}' => a(3, 0, 3, 0, 0), // ╝
        '\u{255E}' => a(1, 1, 0, 3, 0), // ╞
        '\u{255F}' => a(3, 3, 0, 1, 0), // ╟
        '\u{2560}' => a(3, 3, 0, 3, 0), // ╠
        '\u{2561}' => a(1, 1, 3, 0, 0), // ╡
        '\u{2562}' => a(3, 3, 1, 0, 0), // ╢
        '\u{2563}' => a(3, 3, 3, 0, 0), // ╣
        '\u{2564}' => a(0, 1, 3, 3, 0), // ╤
        '\u{2565}' => a(0, 3, 1, 1, 0), // ╥
        '\u{2566}' => a(0, 3, 3, 3, 0), // ╦
        '\u{2567}' => a(1, 0, 3, 3, 0), // ╧
        '\u{2568}' => a(3, 0, 1, 1, 0), // ╨
        '\u{2569}' => a(3, 0, 3, 3, 0), // ╩
        '\u{256A}' => a(1, 1, 3, 3, 0), // ╪
        '\u{256B}' => a(3, 3, 1, 1, 0), // ╫
        '\u{256C}' => a(3, 3, 3, 3, 0), // ╬
        '\u{256D}' => Arc {
            down: true,
            right: true,
        }, // ╭
        '\u{256E}' => Arc {
            down: true,
            right: false,
        }, // ╮
        '\u{256F}' => Arc {
            down: false,
            right: false,
        }, // ╯
        '\u{2570}' => Arc {
            down: false,
            right: true,
        }, // ╰
        '\u{2571}' => Diagonal {
            slash: true,
            backslash: false,
        }, // ╱
        '\u{2572}' => Diagonal {
            slash: false,
            backslash: true,
        }, // ╲
        '\u{2573}' => Diagonal {
            slash: true,
            backslash: true,
        }, // ╳
        '\u{2574}' => a(0, 0, 1, 0, 0), // ╴
        '\u{2575}' => a(1, 0, 0, 0, 0), // ╵
        '\u{2576}' => a(0, 0, 0, 1, 0), // ╶
        '\u{2577}' => a(0, 1, 0, 0, 0), // ╷
        '\u{2578}' => a(0, 0, 2, 0, 0), // ╸
        '\u{2579}' => a(2, 0, 0, 0, 0), // ╹
        '\u{257A}' => a(0, 0, 0, 2, 0), // ╺
        '\u{257B}' => a(0, 2, 0, 0, 0), // ╻
        '\u{257C}' => a(0, 0, 1, 2, 0), // ╼
        '\u{257D}' => a(1, 2, 0, 0, 0), // ╽
        '\u{257E}' => a(0, 0, 2, 1, 0), // ╾
        '\u{257F}' => a(2, 1, 0, 0, 0), // ╿
        _ => return None,
    })
}

/// R1180 §5.41 — the per-cell box-drawing line metrics: integer-snapped centre
/// `(xm, ym)`, light / heavy line thickness `(lw, hw)`, and the double-line
/// rail half-separation `d`. Thickness derives from the smaller cell dimension
/// so a line reads the same weight whether horizontal or vertical.
/// R1181 §5.41 — box-drawing dash duty cycle: each dash covers this fraction of
/// its slot; the remainder is the inter-dash gap (so a triple-dash splits the
/// run into three inked segments with two gaps).
const BOX_DASH_DUTY: f64 = 0.7;

/// R1181 §5.41 — rounded-corner arc radius as a fraction of the smaller cell
/// dimension: a soft quarter-bend that still leaves a short straight stub to
/// each present arm, so the corner reads as an arc rather than a chamfer.
const BOX_ARC_RADIUS_FRACTION: f64 = 0.4;

fn box_line_metrics(cell: KurboRect) -> (f64, f64, f64, f64, f64) {
    let unit = cell.width().min(cell.height());
    let lw = (unit / 8.0).round().max(1.0);
    let hw = (lw * 2.0).max(2.0);
    let d = lw.max(1.0);
    let xm = cell.x0 + (cell.width() / 2.0).round();
    let ym = cell.y0 + (cell.height() / 2.0).round();
    (xm, ym, lw, hw, d)
}

/// R1180 §5.41 — the filled rectangles for a [`BoxGlyph::Arms`] glyph (the
/// straight / junction / double / dashed family). Each present arm runs from
/// its cell edge to the centre cross; when any arm is `double` every arm
/// overshoots the centre by the rail half-separation so the central rail box
/// stays connected, and double arms draw two parallel rails. Integer-snapped
/// bands keep abutting cells gap-free.
///
/// R1181 — returns a stack `([KurboRect; 8], count)` (zero heap allocation in
/// the per-frame paint loop, matching the [`block_element_rects`] sibling). The
/// `8` cap is exact: the densest glyph `╬` (all four arms double) emits two
/// rails × four arms, and a dashed glyph emits at most four segments.
fn box_arm_rects(
    up: u8,
    down: u8,
    left: u8,
    right: u8,
    dash: u8,
    cell: KurboRect,
) -> ([KurboRect; 8], usize) {
    let (xm, ym, lw, hw, d) = box_line_metrics(cell);
    let (x0, y0, x1, y1) = (cell.x0, cell.y0, cell.x1, cell.y1);
    let mut rects = [KurboRect::ZERO; 8];
    let mut n = 0usize;

    if dash != 0 {
        // Each dash covers `BOX_DASH_DUTY` of its slot, centred — the rest is
        // the inter-dash gap. The dashed glyphs are pure single-axis.
        let margin = (1.0 - BOX_DASH_DUTY) / 2.0;
        let t = if up.max(down).max(left).max(right) == 2 {
            hw
        } else {
            lw
        };
        let count = u32::from(dash);
        if left > 0 || right > 0 {
            let yt = (ym - t / 2.0).round();
            let seg = cell.width() / f64::from(count);
            for i in 0..count {
                let sx = x0 + f64::from(i) * seg;
                rects[n] = KurboRect::new(sx + seg * margin, yt, sx + seg * (1.0 - margin), yt + t);
                n += 1;
            }
        } else {
            let xt = (xm - t / 2.0).round();
            let seg = cell.height() / f64::from(count);
            for i in 0..count {
                let sy = y0 + f64::from(i) * seg;
                rects[n] = KurboRect::new(xt, sy + seg * margin, xt + t, sy + seg * (1.0 - margin));
                n += 1;
            }
        }
        return (rects, n);
    }

    let any_double = up == 3 || down == 3 || left == 3 || right == 3;
    let over = if any_double { d } else { 0.0 };
    let push_h = |rects: &mut [KurboRect; 8], n: &mut usize, xa: f64, xb: f64, w: u8| {
        if w == 3 {
            for off in [-d, d] {
                let yt = (ym + off - lw / 2.0).round();
                rects[*n] = KurboRect::new(xa, yt, xb, yt + lw);
                *n += 1;
            }
        } else if w > 0 {
            let t = if w == 2 { hw } else { lw };
            let yt = (ym - t / 2.0).round();
            rects[*n] = KurboRect::new(xa, yt, xb, yt + t);
            *n += 1;
        }
    };
    let push_v = |rects: &mut [KurboRect; 8], n: &mut usize, ya: f64, yb: f64, w: u8| {
        if w == 3 {
            for off in [-d, d] {
                let xt = (xm + off - lw / 2.0).round();
                rects[*n] = KurboRect::new(xt, ya, xt + lw, yb);
                *n += 1;
            }
        } else if w > 0 {
            let t = if w == 2 { hw } else { lw };
            let xt = (xm - t / 2.0).round();
            rects[*n] = KurboRect::new(xt, ya, xt + t, yb);
            *n += 1;
        }
    };
    push_h(&mut rects, &mut n, x0, xm + over, left);
    push_h(&mut rects, &mut n, xm - over, x1, right);
    push_v(&mut rects, &mut n, y0, ym + over, up);
    push_v(&mut rects, &mut n, ym - over, y1, down);
    (rects, n)
}

/// R1180 §5.41 — paint a [`BoxGlyph`] in `brush`: straight/junction families as
/// filled rectangles ([`box_arm_rects`]), rounded corners as a quarter
/// quadratic curve, and diagonals as corner-to-corner strokes.
fn paint_box_drawing(
    out: &mut VelloScene,
    origin: Affine,
    brush: PenikoColor,
    glyph: BoxGlyph,
    cell: KurboRect,
) {
    match glyph {
        BoxGlyph::Arms {
            up,
            down,
            left,
            right,
            dash,
        } => {
            let (rects, count) = box_arm_rects(up, down, left, right, dash, cell);
            for r in &rects[..count] {
                out.fill(Fill::NonZero, origin, brush, None, r);
            }
        }
        BoxGlyph::Arc { down, right } => {
            let (xm, ym, lw, ..) = box_line_metrics(cell);
            let r = (cell.width().min(cell.height()) * BOX_ARC_RADIUS_FRACTION).max(1.0);
            let hx_edge = if right { cell.x1 } else { cell.x0 };
            let hx_inner = if right { xm + r } else { xm - r };
            let vy_edge = if down { cell.y1 } else { cell.y0 };
            let vy_inner = if down { ym + r } else { ym - r };
            let mut path = BezPath::new();
            path.move_to((hx_edge, ym));
            path.line_to((hx_inner, ym));
            // Quadratic through the sharp corner rounds the bend.
            path.quad_to((xm, ym), (xm, vy_inner));
            path.line_to((xm, vy_edge));
            out.stroke(&Stroke::new(lw), origin, brush, None, &path);
        }
        BoxGlyph::Diagonal { slash, backslash } => {
            let (.., lw, _, _) = box_line_metrics(cell);
            let stroke = Stroke::new(lw);
            if slash {
                // bottom-left to top-right
                let line = Line::new((cell.x0, cell.y1), (cell.x1, cell.y0));
                out.stroke(&stroke, origin, brush, None, &line);
            }
            if backslash {
                let line = Line::new((cell.x0, cell.y0), (cell.x1, cell.y1));
                out.stroke(&stroke, origin, brush, None, &line);
            }
        }
    }
}

/// R1181 §5.41 — the **single source of truth** for "is this cell painted as
/// synthesised geometry, not a font glyph". Returns the lone codepoint of
/// `cluster` when it is a single char in the contiguous synthesisable range
/// `U+2500`–`U+259F` — box-drawing (`2500`–`257F`) plus block elements & shades
/// (`2580`–`259F`) — else `None`. This one range check fast-rejects ordinary
/// text / spaces / CJK before any per-classifier work, and gives the three
/// classifiers a `char` so none of them re-extract the cluster.
fn synthesizable_char(cluster: &str) -> Option<char> {
    let mut chars = cluster.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    ('\u{2500}'..='\u{259F}').contains(&c).then_some(c)
}

/// R1179 §5.41 — paint a synthesised cell graphic for `cluster` in `ink`,
/// returning `true` when `cluster` was a synthesised glyph class (so the caller
/// skips the font-glyph path). `ink` is the cell's effective foreground for the
/// main grid pass, or the background for the inverse block-cursor redraw — the
/// one routine serves both, keeping the two emit sites in lock-step.
///
/// The lone-codepoint / range gate is [`synthesizable_char`] (one SSOT); the
/// `char` then dispatches solid Block Element (cell-exact fill) -> shade block
/// (alpha fill) -> box-drawing (R1180). The cell background (pass 1) is already
/// laid, so a shade composites over it as the conventional blend.
fn paint_cell_synthesis(
    out: &mut VelloScene,
    origin: Affine,
    ink: Color,
    cluster: &str,
    cell: KurboRect,
) -> bool {
    let Some(c) = synthesizable_char(cluster) else {
        return false;
    };
    if let Some((rects, count)) = block_element_rects(c, cell) {
        let brush = to_peniko(ink);
        for r in &rects[..count] {
            out.fill(Fill::NonZero, origin, brush, None, r);
        }
        return true;
    }
    if let Some((num, den)) = shade_block_fraction(c) {
        // Ink at `num/den` of its own alpha, filling the whole cell over the
        // already-painted background — the terminal stipple as an alpha blend.
        let a = u8::try_from(u16::from(ink.a) * num / den).unwrap_or(u8::MAX);
        let brush = to_peniko(ink.with_alpha(a));
        out.fill(Fill::NonZero, origin, brush, None, &cell);
        return true;
    }
    if let Some(glyph) = box_drawing(c) {
        paint_box_drawing(out, origin, to_peniko(ink), glyph, cell);
        return true;
    }
    false
}

/// R991 §5.41 §2 #6 — paint one retained [`Scene::TextGrid`] node: the
/// cell-native terminal projection's GUI (Vello) glyph rasterisation.
/// This is the deferred second half of the §5.41 cell-native axis —
/// R972–R978 shipped the cell data model + `scene/snapshot` introspection
/// while the grid stayed paint-opaque; this arm makes it visible.
///
/// Cells are placed by the node-local
/// [`CellMetric`] (R968 ratify) and
/// painted in **two grid-wide passes** — all backgrounds, then all glyphs:
///
/// 1. **Background** — the cell's `bg` [`TermColor`],
///    resolved through the node [`Palette`]
///    ([`ColorTarget::Background`]), fills the whole cell. A
///    [`CellWidth::Trailer`] carries the wide head's colours (R976), so
///    filling every cell's own `bg` paints the head background across both
///    columns with no special case.
/// 2. **Glyph** — the grapheme `cluster` is shaped through the shared
///    [`LayoutCache`] (the same parley → [`vello::Scene::draw_glyphs`] path
///    [`paint_text`] uses — no bespoke rasteriser) and drawn in the
///    resolved `fg` colour at the cell origin. The brush is applied at draw
///    time so the cache key stays colour-independent (one `Layout` per
///    distinct cluster, reused across colours).
///
/// R1013 §5.41 — the two passes are *grid-wide*, not interleaved per cell.
/// A wide head's glyph renders at its natural ~1em advance and overflows into
/// its trailer column; interleaving (fill bg, draw glyph, fill next bg, …)
/// let the trailer's background fill — emitted right after the head glyph —
/// erase that overflow, so CJK / full-width characters read as horizontally
/// "compressed" (their right portion clipped). Completing the whole
/// background layer before any glyph keeps the overflowing head glyph on top
/// of the trailer background. (The TUI backend never showed this: it skips
/// trailers and the terminal renders the wide head across two columns.)
///
/// `reverse` (SGR 7) swaps the effective fg / bg before resolution;
/// `hidden` (SGR 8, conceal) shows only the background — no glyph, no
/// decoration; a [`CellWidth::Trailer`] carries no independent glyph but
/// still paints its (head-inherited) background and decorations.
///
/// R992 §5.41 paints the typographic SGR attributes onto the same effective
/// foreground:
///
/// * **bold** (SGR 1) / **italic** (SGR 3) select the glyph weight / slant
///   ([`FontWeight::BOLD`] / [`FontStyle::Italic`]) for shaping — distinct
///   `Layout` cache entries, so the colour-independent glyph cache is
///   preserved (the brush is still applied at draw time).
/// * **dim** (SGR 2, faint) attenuates the foreground alpha so the ink
///   blends toward the background (the common terminal "half intensity").
/// * **underline** (SGR 4) / **strikethrough** (SGR 9) stroke a horizontal
///   rule spanning the **full cell** at a cell-geometry offset via
///   [`stroke_hrule`] — the terminal convention, so adjacent attributed
///   cells (and a wide head + its trailer) form one continuous rule, and a
///   blank cell still shows its rule. This differs from styled-text
///   decorations ([`paint_decorations`], which span the glyph-run advance
///   at the font metric offset).
///
/// R993 §5.41 paints the [`GridCursor`](pinion_core::term_grid::GridCursor)
/// as an overlay on top of the cells (only when `visible` and within the
/// buffer): **Block** inverts the cell (a fill in the cursor colour with the
/// glyph redrawn in the cell background), **Bar** is a thin vertical beam at
/// the leading edge, and **Underline** is a solid bottom bar drawn thicker
/// than the SGR underline so the two read distinctly; a block / underline
/// cursor on a wide head spans both of its columns. The cursor colour is
/// the cell's effective foreground (a dedicated colour is a deferred
/// `GridCursor` field). `blink` (SGR 5, a timing attribute the TUI backend
/// gets free from the host terminal, R994) is the one remaining Vello slice.
///
/// Font policy: the family is requested as the generic `monospace`
/// keyword, which R1002 routes through `FontFamilyName::Generic` so it
/// resolves to a real fixed-pitch face (pre-R1002 it fell back to the
/// proportional sans-serif generic — the looseness / descender root). The
/// glyph size is fit into the cell at paint time (R1001
/// [`fit_font_size_to_cell`]); a producer that wants `cell_w` to equal the
/// monospace advance sources its [`CellMetric`] from
/// [`pinion_text::LayoutCache::measure_monospace_cell`] (the R968
/// font-derivation hook). R1013 fixed the draw-order overpaint that made a
/// wide head's overflowing glyph read as "compressed". The residual is that
/// this §5.36 parley bridge shapes through `FontContext::new()` (system
/// fallback), whose CJK face is not metric-matched to the Latin half-width,
/// so a wide glyph under-fills its 2-column span (R1013.1 measured ~70.8%,
/// left-aligned) — and it is already at the cell-`h`-maximal size, so it
/// cannot be enlarged without overflowing the cell; horizontal scale /
/// centring to force-fill is rejected (R1002 distortion). The CJK-fill
/// resolution belongs to the §5.37 self-hosted text engine, which supersedes
/// this §5.36 parley bridge — but §5.37 is currently only an OpenType parser
/// (R50.1.x) and has NOT yet decided a fallback-face / metric-matching policy
/// (Nanum Gothic / Noto Sans are §5.37.1 parser *test fixtures*, not yet a
/// decided render font). So this under-fill is a tracked item for the §5.37
/// engine, not an open question to stopgap in this §5.36 bridge — a stopgap
/// would invest
/// in a superseded layer (R1014.1 corrective: the earlier note overclaimed
/// "decided in §5.37").
///
/// R995 §2 #6 — this Vello arm and the TUI `paint_text_grid_inner` must agree
/// on cell *structure* (which cell inks a glyph / reads reversed / forms a
/// wide span; colour stays backend-resolved — pinion palette here vs the host
/// terminal there). That contract is regression-pinned by the shared
/// `pinion_core::test_fixtures::text_grid_consistency_buffer` driven through
/// both backends: the headless-GPU `r995_text_grid_cross_consistency_vello`
/// (pinion-shell) and the exact-buffer `r995_text_grid_cross_consistency_tui`
/// (pinion-tui). The `Block` cursor is the one shape both invert identically;
/// `Bar` / `Underline` render as shaped beams here but as a reverse-block in
/// the character-cell TUI (R994).
/// R1001 §5.41 — the cell-fit monospace font size: the largest integer pixel
/// size whose natural line box fits within `cell_h`.
///
/// `natural_box` is the font's natural line-box height (parley
/// `block_max_coord − block_min_coord`) **measured at a font size of `cell_h`
/// px**. Because the line box scales linearly with font size, the size whose
/// box equals `cell_h` is `cell_h × cell_h / natural_box`; flooring to an
/// integer leaves the sub-pixel remainder as a clearance margin. Returns
/// `cell_h` unchanged when the natural box already fits (no reduction needed)
/// or when `natural_box` is not a usable positive measurement.
///
/// Metric-derived — there is no magic font:cell ratio; the result adapts to
/// whatever monospace family the platform resolves. The motivating bug
/// ([`paint_text_grid`] sizing the glyph to the *full* `cell_h`) overflowed the
/// cell by the font's ascent+descent+leading excess (~1.1–1.2×), clipping
/// descenders on the last grid row and overlapping interior rows.
fn fit_font_size_to_cell(natural_box: f64, cell_h: u32) -> u32 {
    let cell = f64::from(cell_h);
    if !natural_box.is_finite() || natural_box <= cell {
        return cell_h;
    }
    // `box > cell` ⇒ `cell²/box < cell`, so `fitted` lands in `[1, cell_h)`.
    let fitted = (cell * cell / natural_box).floor().max(1.0);
    // Bounded `[1, cell_h)` positive ⇒ the truncating cast is exact on the
    // integer part and never overflows / loses sign.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fitted = fitted as u32;
    fitted
}

/// R1002 §5.41 — the glyph font size (logical px) for a [`TextGridNode`].
///
/// Font-size source of truth: a font-derived grid carries the exact size its
/// cells were measured from ([`TextGridNode::font_size_px`], via
/// `measure_monospace_cell`); paint uses it directly so the rendered advance
/// equals `cell_w` (`== advance(size)`) **by construction** — no re-derivation.
///
/// A producer-picked cell with no font basis (the TUI 8×16 default, or a grid
/// that chose only dimensions) has `None`: fit a font into the cell (R1001).
/// Sizing the font to the *full* `cell_h` overflows parley's natural line box
/// (~1.1–1.2× the em), clipping descenders / overlapping rows; so probe the
/// resolved monospace's natural box at cell height (`probe_style` carries the
/// monospace family) and reduce the font size so that box fits `cell_h`
/// (metric-derived, no magic ratio). `probe_style.font_size_px` is left at the
/// trial cell height; the caller overwrites it with the returned size.
fn grid_glyph_font_size(
    n: &TextGridNode,
    cache: &mut LayoutCache,
    probe_style: &mut TextStyle,
) -> u32 {
    if let Some(size) = n.font_size_px() {
        return size.max(1);
    }
    let cell_h_u = n.cell_metric().cell_h();
    let cell_h = f64::from(cell_h_u);
    // The probe needs the trial size set first; "x" yields the same line-box
    // metrics as any cluster (a font property, not the glyph's).
    probe_style.font_size_px = cell_h_u;
    let natural_box = {
        let probe = cache.layout("x", probe_style, None);
        probe.lines().next().map_or(cell_h, |line| {
            let m = line.metrics();
            f64::from(m.block_max_coord - m.block_min_coord)
        })
    };
    fit_font_size_to_cell(natural_box, cell_h_u)
}

fn paint_text_grid(
    out: &mut VelloScene,
    n: &TextGridNode,
    cache: &mut LayoutCache,
    transform: Affine,
    cursor: CursorPaintFlags,
) {
    let grid = n.cells();
    let palette = n.palette();
    // R991.1 — clip all grid paint to the node's layout rect. The producer's
    // buffer dims and the node's rect-derived winsize are distinct facts that
    // can diverge during an in-flight resize (see `term_grid`); clipping stops
    // an over-large buffer from overdrawing past `rect`, mirroring
    // `paint_text`'s overflow clip.
    let clip_rect = KurboRect::new(
        f64::from(n.rect.x),
        f64::from(n.rect.y),
        f64::from(n.rect.x.saturating_add(n.rect.w)),
        f64::from(n.rect.y.saturating_add(n.rect.h)),
    );
    out.push_clip_layer(Fill::NonZero, transform, &clip_rect);
    // Pass 0 — fill the whole node rect with the palette default background
    // before any cell. The cell area (`cols*cell_w x rows*cell_h`) need not
    // tile `rect` exactly: a consumer that sizes the grid to a continuous
    // pixel rect (the §3 one-way winsize SSOT — cols/rows are *derived* from
    // the layout rect, not chosen to be an exact multiple) leaves a sub-cell
    // gutter on the right / bottom edge. A geometry-only grid (a sized rect
    // with no cells yet — a documented `TextGridNode::new` state, and the
    // transient first frame before a consumer pushes its buffer) is the
    // whole-rect case of the same gap, which is why pass 0 runs *before* the
    // empty-grid early-out below. Real terminal emulators paint the whole
    // widget in the default background and draw cells on top, so the gutter
    // reads as the terminal background; without this it exposes whatever parent
    // surface sits behind the grid (e.g. a splitter Container's `Surface` fill
    // bleeding through). The clip above bounds the fill to `rect`; pass 1's
    // cell backgrounds and pass 2's glyphs draw over it, so every covered cell
    // is visually unchanged (R15.1 / R1028.1 geometry-only completeness).
    out.fill(
        Fill::NonZero,
        transform,
        to_peniko(palette.default_bg()),
        None,
        &clip_rect,
    );
    // An empty (geometry-only) grid has no cells, glyphs, or cursor to paint —
    // pass 0 already laid its background, so balance the clip and return.
    if grid.is_empty() {
        out.pop_layer();
        return;
    }
    let metric = n.cell_metric();
    // R1542 §5.41 §2 #6 — paint at most the grid the NODE holds, mirroring the
    // TUI backend's identical clamp (`pinion_tui::paint`). Until R1542 this
    // arm relied on the `rect` clip alone, which is equivalent only while the
    // node's winsize is derived FROM that rect. It stopped being equivalent in
    // two ways: a rect whose width is not a whole number of cells left the
    // GUI painting a sliver of one more column than the TUI showed, and a
    // producer-declared winsize (R1542) can be narrower than the rect on
    // purpose — where the cells beyond it are not the grid at all, they are
    // the "sub-grid margin" R1028's fill is responsible for.
    let paint_cols = grid.cols().min(n.cols());
    let paint_rows = grid.rows().min(n.rows());
    let cell_w = f64::from(metric.cell_w());
    let cell_h = f64::from(metric.cell_h());
    // Glyphs paint in the grid-local frame translated to the node's
    // layout-resolved origin, composed with the inherited transform (e.g.
    // a parent `Scene::Scroll`'s shifted child transform) — exactly like
    // [`paint_text`].
    let origin = node_local_transform(transform, n.rect);
    // The family is the generic monospace class (R1002 typed
    // `with_generic_family` — resolves to a real fixed-pitch face; see the
    // font-policy note above). [`grid_glyph_font_size`] resolves the size
    // (measured SSOT vs cell-fit); `line_height = cell_h` pins the line box to
    // the cell so the glyph centres vertically without clipping a descender.
    let mut style = TextStyle::new().with_generic_family(GenericFontFamily::Monospace);
    style.font_size_px = grid_glyph_font_size(n, cache, &mut style);
    style.line_height = LineHeight::Px(metric.cell_h());
    // SGR 4 underline / SGR 9 strikethrough are rules of this pen width.
    // Cell height is loop-invariant so the width is hoisted; the per-cell Y
    // offsets depend on the cell origin and are computed inside.
    let rule_w = (cell_h / 16.0).max(1.0);
    // Pass 1 — every cell's opaque background. R1013 §5.41: backgrounds are
    // laid down for the whole grid *before* any glyph, so a wide head glyph
    // that overflows its column into the trailer (drawn in pass 2) is not
    // erased by the trailer's own background fill. SGR 7 reverse swaps the
    // effective fg / bg before resolution; a [`CellWidth::Trailer`] carries
    // the wide head's colours (R976), so filling each cell's own `bg` paints
    // the head background across both columns with no special case.
    for row in 0..paint_rows {
        for col in 0..paint_cols {
            let Some(cell) = grid.cell(col, row) else {
                continue;
            };
            let (_, bg_term) = effective_terms(cell);
            let bg = palette.resolve(bg_term, ColorTarget::Background);
            let (cx, cy) = metric.cell_to_px(col, row);
            let rect = KurboRect::new(cx, cy, cx + cell_w, cy + cell_h);
            out.fill(Fill::NonZero, origin, to_peniko(bg), None, &rect);
        }
    }
    // Pass 2 — every cell's glyph + SGR decorations, painted on top of the
    // completed background layer. This is the draw-order that keeps an
    // overflowing wide head glyph (its natural ~1em advance spilling into the
    // trailer column) visible: it now lands over the trailer background
    // instead of under it.
    for row in 0..paint_rows {
        for col in 0..paint_cols {
            let Some(cell) = grid.cell(col, row) else {
                continue;
            };
            // SGR 8 hidden (conceal): the cell shows only its background —
            // no glyph, no decoration.
            if cell.attrs.hidden {
                continue;
            }
            // SGR 7 reverse: swap the effective fg / bg before palette
            // resolution. The effective foreground for this cell's ink (glyph
            // + decorations): palette-resolved, then SGR 2 dim attenuates the
            // alpha so the ink blends toward the background.
            let (fg_term, _) = effective_terms(cell);
            let mut fg = palette.resolve(fg_term, ColorTarget::Foreground);
            if cell.attrs.dim {
                // Half the alpha ~ the common terminal "faint" intensity.
                fg = fg.with_alpha(fg.a / 2);
            }
            let fg_brush = to_peniko(fg);
            let (cx, cy) = metric.cell_to_px(col, row);
            // Glyph, unless suppressed. A Trailer carries no independent glyph
            // (R976); an all-whitespace cluster has nothing to ink. SGR 1 bold
            // / SGR 3 italic pick the weight / slant — distinct `Layout` cache
            // entries, so the colour-independent glyph cache stays intact (the
            // brush is still applied at draw time).
            // R1178/R1179 §5.41 — a synthesised cell graphic (solid block,
            // shade, or R1180 box-drawing) paints as cell-exact geometry in the
            // effective foreground instead of a font glyph, so block / box art
            // tiles its cells gap-free (the fitted glyph's bearing margin left
            // the inter-cell gaps R1002 pins). Everything else falls through to
            // the glyph path unchanged.
            let cell_rect = KurboRect::new(cx, cy, cx + cell_w, cy + cell_h);
            if paint_cell_synthesis(out, origin, fg, &cell.cluster, cell_rect) {
                // synthesised geometry — no glyph
            } else if has_glyph_cluster(cell) {
                let glyph_transform = origin * Affine::translate((cx, cy));
                draw_cell_glyph(out, cache, &style, cell, glyph_transform, fg_brush);
            }
            // Underline (R1399: the SGR 4:x style — single / double / curly /
            // dotted / dashed — drawn in its own SGR-58 colour when set, else
            // the effective foreground) + strikethrough rules spanning the
            // full cell (so adjacent attributed cells, and a wide head +
            // trailer, form one continuous rule; a blank cell still shows its
            // rule).
            if cell.attrs.underline.is_on() {
                // An explicit underline colour (SGR 58) tints the rule at full
                // intensity; the SGR-59 default follows the (dim-attenuated)
                // effective foreground so a plain underline still fades with
                // `dim`.
                let ul_brush = cell.underline_color.map_or(fg_brush, |uc| {
                    to_peniko(palette.resolve(uc, ColorTarget::Foreground))
                });
                paint_underline(
                    out,
                    origin,
                    ul_brush,
                    cx,
                    cx + cell_w,
                    cy + cell_h,
                    rule_w,
                    cell.attrs.underline,
                );
            }
            if cell.attrs.strikethrough {
                let y = cy + cell_h * 0.5;
                stroke_hrule(out, origin, fg_brush, cx, cx + cell_w, y, rule_w);
            }
        }
    }
    // Cursor overlay (R993) — drawn after the cells so it sits on top, inside
    // the same clip layer. Split into its own pass (the cursor's effective-
    // colour + per-shape geometry is a concern distinct from the cell grid).
    paint_grid_cursor(
        out,
        grid,
        (paint_cols, paint_rows),
        metric,
        palette,
        cache,
        &style,
        origin,
        cursor,
    );
    out.pop_layer();
}

/// R993 §5.41 — paint the [`TextGridNode`] cursor overlay on top of the already
/// completed cell layers, sharing the caller's clip. Split out of
/// [`paint_text_grid`] so that painter stays a single cohesive pass sequence
/// (pass 0 default-bg fill, pass 1 cell backgrounds, pass 2 glyphs) and the
/// cursor — a distinct concern (effective-colour resolution + per-shape
/// geometry) — owns its own routine. `cell_w` / `cell_h` are re-derived from
/// `metric` rather than threaded in, keeping the argument list small.
///
/// The producer reports the effective cursor (R975); only a visible cursor
/// whose cell falls within the buffer paints — an out-of-buffer position is a
/// transient resize artefact and is skipped.
///
/// R1427 §5.41 §5.39 — `flags.focused` gates the fill-vs-hollow render (checked
/// only AFTER [`GridCursor::shown_this_phase`](pinion_core::term_grid::GridCursor::shown_this_phase),
/// so a hidden / off-phase cursor is never resurrected): `false` (the window
/// lacks OS focus) draws a HOLLOW outline box overriding the shape; `true` draws
/// the filled block / bar / underline. Focus-hollow is a function of focus, not
/// blink — a steady cursor goes hollow too.
///
/// R1426 §5.41 — `flags.blink_on` is the render-time blink phase: a cursor in
/// the blinking DECSCUSR mode ([`GridCursor::blink`](pinion_core::term_grid::GridCursor::blink))
/// paints only when the phase is on; a steady cursor ignores it. Gated through
/// the shared [`GridCursor::shown_this_phase`](pinion_core::term_grid::GridCursor::shown_this_phase)
/// predicate (the same one the pinion-tui painter uses) so the two backends
/// can never drift. The phase is never folded into `visible`: skipping the
/// overlay on the off-phase restores the plain cell (the normal glyph the cell
/// pass already painted reads through) for every shape — the real terminal
/// off-phase — while `scene/snapshot` still reports `visible`/`blink` as data.
#[allow(clippy::too_many_arguments)]
fn paint_grid_cursor(
    out: &mut VelloScene,
    grid: &GridBuffer,
    painted: (u16, u16),
    metric: CellMetric,
    palette: Palette,
    cache: &mut LayoutCache,
    style: &TextStyle,
    origin: Affine,
    flags: CursorPaintFlags,
) {
    let cursor = grid.cursor();
    // R1542 — bounded by what this frame PAINTED, not by the buffer alone: a
    // cursor outside the node's winsize sits in the sub-grid margin, where
    // there is no cell for it to invert and R1028's fill owns the pixels.
    let (painted_cols, painted_rows) = painted;
    if !(cursor.shown_this_phase(flags.blink_on)
        && cursor.col < painted_cols
        && cursor.row < painted_rows)
    {
        return;
    }
    let cell_w = f64::from(metric.cell_w());
    let cell_h = f64::from(metric.cell_h());
    let (cx, cy) = metric.cell_to_px(cursor.col, cursor.row);
    let cur_cell = grid.cell(cursor.col, cursor.row);
    // The cursor's default ink is the cell's effective (reverse-honoured)
    // foreground — its ink colour — at full intensity (no dim: the cursor is a
    // prominent UI accent, not faint text). An absent cell (resize) falls back
    // to the palette default fg / bg. `bg_term` is the effective background the
    // block cursor redraws the glyph in so the character reads through.
    let (fg_term, bg_term) =
        cur_cell.map_or((TermColor::Default, TermColor::Default), effective_terms);
    // R1424 §5.41 — an explicit OSC-12 cursor colour ([`GridCursor::cursor_color`])
    // overrides the cell-derived ink; `None` keeps the effective-foreground
    // default, so a producer that sets no cursor colour renders exactly as
    // before. The OSC-12 colour is an absolute `Color` (no palette resolution).
    let cursor_color = to_peniko(
        cursor
            .cursor_color
            .unwrap_or_else(|| palette.resolve(fg_term, ColorTarget::Foreground)),
    );
    // A block / underline cursor on a wide head spans both of its columns,
    // matching the glyph (and the TUI, where the reversed head renders two
    // columns wide). The bar stays a single leading-edge beam.
    let span_w = if matches!(cur_cell, Some(c) if c.width == CellWidth::Wide) {
        2.0 * cell_w
    } else {
        cell_w
    };
    // R1427 §5.41 §5.39 — an unfocused window draws its cursor as a HOLLOW
    // outline box, overriding the shape (block / bar / underline all become the
    // outline), the universal focus indicator every real terminal uses (xterm
    // open-box, VTE unfilled rect, alacritty `HollowBlock`, kitty
    // `cursor_shape_unfocused=hollow`, Windows Terminal, iTerm2). It is a
    // function of *focus*, not blink, so a steady cursor goes hollow too; the
    // stop-blink gate (`grid_cursor_blink_on` steady-on when unfocused) means a
    // blinking cursor resolves here to a steady hollow box. The glyph reads
    // normally underneath (no inverse redraw — only the outline is drawn). The
    // outline spans both columns on a wide head (`span_w`) and is inset by half
    // the stroke so it stays inside the cell and never bleeds into a neighbour.
    if !flags.focused {
        let sw = (cell_w / 8.0).max(1.0);
        let half = sw / 2.0;
        let rect = KurboRect::new(cx + half, cy + half, cx + span_w - half, cy + cell_h - half);
        out.stroke(&Stroke::new(sw), origin, cursor_color, None, &rect);
        return;
    }
    match cursor.shape {
        CursorShape::Block => {
            // Inverse block: fill the cursor span in the cursor colour, then
            // redraw the glyph in the cell background so the character reads
            // through (the conventional terminal block cursor).
            let rect = KurboRect::new(cx, cy, cx + span_w, cy + cell_h);
            out.fill(Fill::NonZero, origin, cursor_color, None, &rect);
            if let Some(c) = cur_cell {
                if !c.attrs.hidden {
                    let bg_color = palette.resolve(bg_term, ColorTarget::Background);
                    let cell_rect = KurboRect::new(cx, cy, cx + cell_w, cy + cell_h);
                    // R1178/R1179 — a synthesised graphic (block / shade / box)
                    // redraws its geometry in the cell background, so an inverse
                    // block cursor reads through it exactly as it does a glyph.
                    if paint_cell_synthesis(out, origin, bg_color, &c.cluster, cell_rect) {
                        // synthesised in bg — no glyph redraw
                    } else if has_glyph_cluster(c) {
                        let glyph_transform = origin * Affine::translate((cx, cy));
                        draw_cell_glyph(out, cache, style, c, glyph_transform, to_peniko(bg_color));
                    }
                }
            }
        }
        CursorShape::Bar => {
            // A thin vertical beam at the cell's leading edge.
            let bar_w = (cell_w / 8.0).max(1.0);
            let rect = KurboRect::new(cx, cy, cx + bar_w, cy + cell_h);
            out.fill(Fill::NonZero, origin, cursor_color, None, &rect);
        }
        CursorShape::Underline => {
            // A solid bar along the cell bottom — deliberately thicker than the
            // SGR underline rule so the cursor reads distinctly from an
            // underlined character.
            let uc_h = (cell_h / 8.0).max(2.0);
            let rect = KurboRect::new(cx, cy + cell_h - uc_h, cx + span_w, cy + cell_h);
            out.fill(Fill::NonZero, origin, cursor_color, None, &rect);
        }
    }
}

/// Emit one Vello filled-rectangle path for a pinion (`Rect`, `Color`,
/// `corner_radius`) triple. Transparent fills are skipped (matches the
/// pre-R46.3.1 `paint_filled_rect` early-exit).
///
/// R639 §5.16 §5.2 — `corner_radius == 0` paints a sharp [`KurboRect`]
/// (the legacy zero-cost path used by every Container/Box that does
/// not set `BoxStyle.corner_radius`). `corner_radius > 0` paints a
/// [`KurboRoundedRect`] with a single uniform radius applied to all
/// four corners. `kurbo::RoundedRect::from_rect` auto-clamps the
/// radius to `min(width, height) / 2`, so an over-large radius (M3
/// Filled Button's `radius: 100` against a 40-px-tall rect) naturally
/// resolves to a pill shape without an explicit caller-side clamp.
///
/// Per-corner asymmetric radii (Figma `rectangleCornerRadii: [tl, tr,
/// br, bl]`) deferred to a follow-up round once the first asymmetric
/// binding lands — `kurbo::RoundedRectRadii` already supports the
/// shape, this slice only wires the single-radius surface
/// [`BoxStyle.corner_radius`] exposes today.
fn fill_rect(out: &mut VelloScene, r: Rect, fill: Color, corner_radius: u32, transform: Affine) {
    if fill == Color::TRANSPARENT {
        return;
    }
    let x0 = f64::from(r.x);
    let y0 = f64::from(r.y);
    let x1 = f64::from(r.x.saturating_add(r.w));
    let y1 = f64::from(r.y.saturating_add(r.h));
    let peniko_fill = to_peniko(fill);
    if corner_radius == 0 {
        let rect = KurboRect::new(x0, y0, x1, y1);
        out.fill(Fill::NonZero, transform, peniko_fill, None, &rect);
    } else {
        let rounded = KurboRoundedRect::new(x0, y0, x1, y1, f64::from(corner_radius));
        out.fill(Fill::NonZero, transform, peniko_fill, None, &rounded);
    }
}

/// R710 §5.50 — paint every [`BoxStyle::shadows`] entry behind a box,
/// in list order (back-to-front), via Vello's native gaussian-blurred
/// rounded-rect. The shadow silhouette is `r` translated by the
/// shadow's `(offset_x, offset_y)` and inflated by `spread`; the corner
/// radius tracks the box's `corner_radius` grown by `spread`; the
/// gaussian std-dev is `blur / 2` (the CSS `box-shadow` convention).
///
/// Called *before* [`fill_box_bg`] so the opaque fill composites over
/// the shadow's interior. Like every other leaf draw it is issued into
/// the caller's fresh sub-scene before any child `append`, preserving
/// the R706 "out receives appends only" invariant.
fn paint_box_shadows(out: &mut VelloScene, r: Rect, style: &BoxStyle, transform: Affine) {
    for shadow in &style.shadows {
        if shadow.color == Color::TRANSPARENT {
            continue;
        }
        let spread = f64::from(shadow.spread);
        let x0 = f64::from(r.x) + f64::from(shadow.offset_x) - spread;
        let y0 = f64::from(r.y) + f64::from(shadow.offset_y) - spread;
        let x1 = f64::from(r.x.saturating_add(r.w)) + f64::from(shadow.offset_x) + spread;
        let y1 = f64::from(r.y.saturating_add(r.h)) + f64::from(shadow.offset_y) + spread;
        // A spread more negative than half the box collapses the rect —
        // nothing to cast.
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let radius = (f64::from(style.corner_radius) + spread).max(0.0);
        let std_dev = f64::from(shadow.blur) / 2.0;
        let rect = KurboRect::new(x0, y0, x1, y1);
        out.draw_blurred_rounded_rect(transform, rect, to_peniko(shadow.color), radius, std_dev);
    }
}

/// R721 §5.16 — convert a pinion [`PathPoint`] to a Vello [`KurboPoint`].
///
/// R1358 — the point is in the node's own frame (relative to
/// [`PathNode::rect`]'s origin), NOT device pixels: [`paint_path`] carries
/// the rect origin in the transform it fills with, so the lowering here is
/// a pure unit conversion and must stay one. Before R1358 this doc claimed
/// the point was "absolute device-pixel, the same space as `PathNode::rect`"
/// — the exact proposition R1358 falsified.
fn path_point(p: PathPoint) -> KurboPoint {
    KurboPoint::new(f64::from(p.x), f64::from(p.y))
}

/// R721 §5.16 — map a pinion [`StrokeCap`] to a Vello [`KurboCap`] 1:1.
fn to_kurbo_cap(cap: StrokeCap) -> KurboCap {
    match cap {
        StrokeCap::Round => KurboCap::Round,
        StrokeCap::Square => KurboCap::Square,
        // `Butt` and any future `#[non_exhaustive]` variant fall back to
        // the CSS / kurbo default flat cap.
        _ => KurboCap::Butt,
    }
}

/// R740 §5.16 — paint a [`Scene::Image`]
/// leaf. Resolves `node.source` through the decode-once
/// [`ImageCache`] (a missing / undecodable source paints nothing — the
/// same graceful skip the pre-R740 no-op gave, but only for genuinely
/// broken sources), then places the decoded image into `node.rect`
/// according to the [`Fit`] policy:
///
/// * [`Fit::Fill`] — non-uniform stretch to the rect (default).
/// * [`Fit::Contain`] — uniform scale to fit *inside* the rect, centred
///   (letterboxed).
/// * [`Fit::Cover`] — uniform scale to *cover* the rect, centred, clipped
///   to the rect (overflow cropped).
/// * [`Fit::Tile`] — repeat at natural size across the rect.
///
/// `style.tint` (a multiply recolour) is a deferred additive axis — it
/// needs a blend layer and has no consumer yet; the buffer is painted
/// untinted for now.
fn paint_image(
    out: &mut VelloScene,
    node: &ImageNode,
    image_cache: &mut ImageCache,
    transform: Affine,
) {
    let Some(data) = image_cache.resolve(&node.source) else {
        return;
    };
    let (iw, ih) = (f64::from(data.width), f64::from(data.height));
    let r = node.rect;
    let (rw, rh) = (f64::from(r.w), f64::from(r.h));
    if iw <= 0.0 || ih <= 0.0 || rw <= 0.0 || rh <= 0.0 {
        return;
    }
    let (rx, ry) = (f64::from(r.x), f64::from(r.y));

    if matches!(node.style.fit, Fit::Tile) {
        // Repeat the image at natural size; the brush transform anchors
        // the tiling origin at the rect's top-left.
        let brush = ImageBrush::new(data).with_extend(PenikoExtend::Repeat);
        let shape = KurboRect::new(rx, ry, rx + rw, ry + rh);
        out.fill(
            Fill::NonZero,
            transform,
            &PenikoBrush::Image(brush),
            Some(Affine::translate((rx, ry))),
            &shape,
        );
        return;
    }

    // Fill / Contain / Cover — a single placement via `draw_image`.
    let (sx, sy) = match node.style.fit {
        Fit::Contain => {
            let s = (rw / iw).min(rh / ih);
            (s, s)
        }
        Fit::Cover => {
            let s = (rw / iw).max(rh / ih);
            (s, s)
        }
        // `Fit` is `#[non_exhaustive]`; Fill + any future variant stretch.
        _ => (rw / iw, rh / ih),
    };
    // Centre the scaled image in the rect (a no-op for Fill).
    let ox = rx + (rw - iw * sx) / 2.0;
    let oy = ry + (rh - ih * sy) / 2.0;
    let place = transform * Affine::new([sx, 0.0, 0.0, sy, ox, oy]);

    // Cover overflows the rect → clip so the overflow is cropped.
    let clip = matches!(node.style.fit, Fit::Cover);
    if clip {
        out.push_clip_layer(
            Fill::NonZero,
            transform,
            &KurboRect::new(rx, ry, rx + rw, ry + rh),
        );
    }
    out.draw_image(&ImageBrush::new(data), place);
    if clip {
        out.pop_layer();
    }
}

/// R721 §5.16 — rasterize a [`Scene::Path`] leaf: lower its
/// `Vec<PathCommand>` into a Vello [`BezPath`], then fill the closed
/// region (non-zero winding — the CSS / SVG default) with either the
/// R722 `style.gradient` when present or the solid `style.fill`, and
/// stroke the outline with `style.stroke`. All
/// [`PathStyle`](pinion_core::style::PathStyle) arms are independently
/// optional, so a fill-only, stroke-only, or empty style each paints
/// only what it carries; an empty command stream is a no-op. Issued
/// into the caller's fresh sub-scene before any child `append`,
/// preserving the R706 "out receives appends only" invariant.
///
/// R1358 — both the geometry and the gradient are
/// [`PathNode::rect`](pinion_core::scene::PathNode::rect)-relative: the
/// node's resolved rect origin enters as a `translate` on the paint
/// transform, so layout alone positions the path. Before R1358 the
/// commands were window-absolute while the gradient was already
/// rect-relative — the two halves of one node disagreed, and a path
/// could not be laid out by flex at all.
///
/// (This paragraph pair sat on [`paint_image`] until R1358: the R740
/// image text was appended to the same doc block, orphaning the R721
/// description from the fn it describes and leaving `paint_path`
/// undocumented. Found by adding to it.)
fn paint_path(out: &mut VelloScene, node: &PathNode, transform: Affine) {
    if node.commands.is_empty() {
        return;
    }
    let mut path = BezPath::new();
    for cmd in &node.commands {
        match *cmd {
            PathCommand::MoveTo(p) => path.move_to(path_point(p)),
            PathCommand::LineTo(p) => path.line_to(path_point(p)),
            PathCommand::CurveTo { c1, c2, end } => {
                path.curve_to(path_point(c1), path_point(c2), path_point(end));
            }
            PathCommand::Close => path.close_path(),
            // `PathCommand` is `#[non_exhaustive]`; an unrecognised
            // future command is skipped rather than mis-rastered.
            _ => {}
        }
    }
    // R1358 — compose `transform * translate(rect.{x,y})` so a
    // rect-local `(0, 0)` command lands at the node's resolved
    // top-left, exactly as `paint_immediate_mode_node` places its
    // viewport-local driver. The path is positioned by layout's
    // `rect` output; the commands never carry window coordinates.
    let local_transform = node_local_transform(transform, node.rect);
    // The gradient's UV geometry anchors to the node's rect, which in
    // the local frame is at the origin — passing `node.rect` here
    // (whose x/y are already applied by `local_transform`) would offset
    // the gradient twice. `gradient_brush` is shared with the Box arm,
    // where the rect IS the paint frame, so the rebase belongs here.
    let local_rect = Rect::new(0, 0, node.rect.w, node.rect.h);
    // Fill: a gradient (R722) overrides the solid fill when present,
    // mirroring `fill_box_bg`'s Box gradient-over-solid precedence.
    if let Some(gradient) = &node.style.gradient {
        let brush = gradient_brush(gradient, local_rect);
        out.fill(Fill::NonZero, local_transform, &brush, None, &path);
    } else if let Some(fill) = node.style.fill
        && fill != Color::TRANSPARENT
    {
        out.fill(Fill::NonZero, local_transform, to_peniko(fill), None, &path);
    }
    if let Some(stroke) = node.style.stroke
        && stroke.width > 0
        && stroke.color != Color::TRANSPARENT
    {
        let kurbo_stroke = Stroke::new(f64::from(stroke.width)).with_caps(to_kurbo_cap(stroke.cap));
        out.stroke(
            &kurbo_stroke,
            local_transform,
            to_peniko(stroke.color),
            None,
            &path,
        );
    }
}

/// R1511 §5.16 §2 #6 — paint everything a [`BoxStyle`] says about the rect
/// it decorates, in stacking order: the [`BoxStyle::shadows`] behind it, the
/// background ([`fill_box_bg`]), then the [`BoxStyle::border`] stroke on top.
/// `solid` is the caller-resolved fill (see [`fill_box_bg`]).
///
/// Both the `Scene::Box` and the `Scene::Container` arms of BOTH walkers
/// ([`to_vello_inner`] and [`to_vello_cached_inner`]) route through here, so
/// the two node types cannot disagree about what a `BoxStyle` means. Until
/// R1511 the four arms open-coded this sequence and only the two `Box` ones
/// stroked the border, so a border declared on a container reached the TUI
/// (`pinion_tui::paint::paint_container`) and the PDF projector but never the
/// GUI — a §2 #6 divergence across renderers of one canonical scene, and one
/// that no test could see because the drop was silent.
///
/// A container strokes its border BEFORE recursing into children, so a child
/// laid out over the edge paints on top of it. That is the order the TUI
/// walker documents and the PDF walker emits, and it is what CSS does with a
/// border and in-flow content.
fn paint_box_decoration(
    out: &mut VelloScene,
    r: Rect,
    style: &BoxStyle,
    solid: Color,
    transform: Affine,
) {
    paint_box_shadows(out, r, style, transform);
    fill_box_bg(out, r, style, solid, transform);
    if let Some(border) = style.border {
        stroke_rect(out, r, border, transform);
    }
}

/// R708 §5.50 — paint a Box / Container background: the
/// [`BoxStyle::gradient`] overlay when present, otherwise the solid
/// `solid` colour. `solid` is the caller-resolved fill (a `Box`'s
/// `fill_hook` override or `style.fill`; a `Container`'s `style.fill`),
/// so a gradient takes precedence over the solid only when explicitly
/// set — mirroring Flutter's `BoxDecoration { color, gradient }`.
fn fill_box_bg(out: &mut VelloScene, r: Rect, style: &BoxStyle, solid: Color, transform: Affine) {
    if let Some(gradient) = &style.gradient {
        fill_rect_gradient(out, r, gradient, style.corner_radius, transform);
    } else {
        fill_rect(out, r, solid, style.corner_radius, transform);
    }
}

/// R708 §5.50 — lower a pinion [`Gradient`] (box-relative UV geometry)
/// onto `r` and fill the rect / rounded-rect with it. UV coordinates
/// are mapped to absolute device coordinates using `r`'s origin + size
/// (so `(0,0)` = top-left corner, `(1,1)` = bottom-right); a radial
/// `radius` is taken as a fraction of the shorter side. Stops and the
/// [`Extend`](pinion_core::style::Extend) mode map 1:1 onto peniko.
fn fill_rect_gradient(
    out: &mut VelloScene,
    r: Rect,
    gradient: &Gradient,
    corner_radius: u32,
    transform: Affine,
) {
    let brush = gradient_brush(gradient, r);
    let x0 = f64::from(r.x);
    let y0 = f64::from(r.y);
    let x1 = x0 + f64::from(r.w);
    let y1 = y0 + f64::from(r.h);
    if corner_radius == 0 {
        let rect = KurboRect::new(x0, y0, x1, y1);
        out.fill(Fill::NonZero, transform, &brush, None, &rect);
    } else {
        let rounded = KurboRoundedRect::new(x0, y0, x1, y1, f64::from(corner_radius));
        out.fill(Fill::NonZero, transform, &brush, None, &rounded);
    }
}

/// R1358.1 §5.16 — compose `parent` with `origin`'s translation: the
/// **node-local paint frame** every leaf shares that positions its content
/// by its own rect. The leaf authors content at `(0, 0)`; the returned
/// transform lands it at `origin` within the parent's frame, so an
/// ancestor's contribution (a [`Scene::Scroll`]'s shifted child transform,
/// a `HiDPI` scale) composes for free and the leaf never learns about it.
///
/// Lifted at the 5th consumer, not the 2nd: the immediate-mode viewport
/// (R681), the text-grid glyph frame (R991), both `paint_text` arms
/// (R51.188 / §5.37), and — since R1358 — `Scene::Path`'s commands. They
/// had drifted into five identical spellings of one rule.
fn node_local_transform(parent: Affine, origin: Rect) -> Affine {
    parent * Affine::translate((f64::from(origin.x), f64::from(origin.y)))
}

/// R722 §5.50 — build a peniko gradient [`PenikoBrush`] from a pinion
/// [`Gradient`] whose box-relative UV geometry is anchored to `r`
/// (`(0,0)` = top-left, `(1,1)` = bottom-right; a radial `radius` is a
/// fraction of the shorter side). Shared by [`fill_rect_gradient`]
/// (Box / Container fills) and [`paint_path`] (R721 vector paths) so
/// the gradient lowering is single-source — only the filled *shape*
/// (rect vs `BezPath`) differs.
///
/// # Caller contract (R1358)
///
/// **`r` must be expressed in the same frame as the transform you fill
/// with.** The brush bakes `r`'s origin into absolute brush coordinates, so
/// the origin must not also be in the transform — that double-offsets the
/// gradient, silently and only for gradient fills. The two callers differ
/// precisely here, and both are correct:
///
/// * [`fill_rect_gradient`] fills with the inherited `transform` and passes
///   the window-space rect.
/// * [`paint_path`] fills with `transform * translate(rect.xy)` and so
///   passes a rect at the ORIGIN (`Rect::new(0, 0, w, h)`).
///
/// A third consumer picks one of those two shapes; there is no third.
fn gradient_brush(gradient: &Gradient, r: Rect) -> PenikoBrush {
    let x0 = f64::from(r.x);
    let y0 = f64::from(r.y);
    let w = f64::from(r.w);
    let h = f64::from(r.h);
    let uv = |u: f32, v: f32| KurboPoint::new(x0 + f64::from(u) * w, y0 + f64::from(v) * h);

    let mut peniko_gradient = match gradient.kind {
        GradientKind::Linear { start, end } => {
            PenikoGradient::new_linear(uv(start.0, start.1), uv(end.0, end.1))
        }
        GradientKind::Radial { center, radius } => {
            // Radius is a fraction of the shorter side, in device px.
            #[allow(clippy::cast_possible_truncation)]
            let radius_px = (f64::from(radius) * w.min(h)) as f32;
            PenikoGradient::new_radial(uv(center.0, center.1), radius_px)
        }
    };
    peniko_gradient = peniko_gradient.with_extend(to_peniko_extend(gradient.extend));
    let stops: Vec<(f32, PenikoColor)> = gradient
        .stops
        .iter()
        .map(|stop| (stop.offset, to_peniko(stop.color)))
        .collect();
    peniko_gradient = peniko_gradient.with_stops(stops.as_slice());
    PenikoBrush::Gradient(peniko_gradient)
}

/// R708 §5.50 — map a pinion [`Extend`](pinion_core::style::Extend) to
/// peniko's `Extend` (1:1; the wildcard covers any future
/// `#[non_exhaustive]` variant with the `Pad` default).
fn to_peniko_extend(extend: pinion_core::style::Extend) -> PenikoExtend {
    use pinion_core::style::Extend;
    match extend {
        Extend::Repeat => PenikoExtend::Repeat,
        Extend::Reflect => PenikoExtend::Reflect,
        Extend::Pad | _ => PenikoExtend::Pad,
    }
}

/// Emit one Vello stroke for a pinion [`Border`]. Vello strokes are
/// path-centered; the [`BorderPlacement`] determines whether we inset
/// (Inside, legacy softbuffer), keep the stroke on the path (Center,
/// Vello-native), or outset (Outside, CSS content-box).
fn stroke_rect(out: &mut VelloScene, r: Rect, border: Border, transform: Affine) {
    if border.width == 0 {
        return;
    }
    let w = f64::from(border.width);
    // Signed offset of the stroke's path centre relative to the rect
    // edge — positive moves inward (Inside), zero leaves on edge
    // (Center), negative moves outward (Outside).
    let offset = match border.placement {
        BorderPlacement::Center => 0.0,
        BorderPlacement::Outside => -(w / 2.0),
        // Inside (R46.3.2 default — legacy softbuffer compatibility)
        // plus any future #[non_exhaustive] variant: conservative
        // inset geometry. Listing Inside under the wildcard rather
        // than as its own arm satisfies clippy::match_same_arms
        // without losing forward-compat coverage.
        BorderPlacement::Inside | _ => w / 2.0,
    };
    let rect = KurboRect::new(
        f64::from(r.x) + offset,
        f64::from(r.y) + offset,
        f64::from(r.x.saturating_add(r.w)) - offset,
        f64::from(r.y.saturating_add(r.h)) - offset,
    );
    out.stroke(
        &Stroke::new(w),
        transform,
        to_peniko(border.color),
        None,
        &rect,
    );
}

/// Emit one Vello glyph run per parley [`GlyphRun`](pinion_text::parley::GlyphRun) shaped from
/// `t.content` + `t.style` (R47.3 §5.36 + R47.6 Figma-fidelity wire).
///
/// The text origin is `(t.rect.x, t.rect.y)`; `t.rect.w > 0` wraps at
/// that pixel width, `w == 0` flows on a single unbounded line.
///
/// R47.6 decoration: when [`TextStyle::decoration`] enables underline
/// or strikethrough, parley populates each [`GlyphRun`](pinion_text::parley::GlyphRun)'s style with a
/// `Decoration<Color>`. We stroke a horizontal [`Line`] at the
/// font-metric-derived offset spanning the run's advance.
///
/// R47.6 overflow: [`TextOverflow::Clip`] wraps the whole emit in a
/// Vello clip layer keyed to `t.rect`; out-of-rect glyphs are clipped
/// before composition. [`TextOverflow::Ellipsis`] silently falls back
/// to `Clip` — parley 0.9 does not expose a native line-truncation
/// API, so the visual result is the same as `Clip` until R47.x lands
/// the custom truncation pass. [`TextOverflow::Visible`] (default)
/// skips the clip wrap entirely.
/// R1068 §5.37 — is this `Scene::Text` leaf *eligible* for the opt-in self-hosted
/// paint arm? These are the NECESSARY conditions under which the arm can paint
/// the text so it registers exactly with what parley laid out — every one is a
/// case the arm would otherwise render differently from parley:
///
/// - **single style** (`runs` empty) — styled runs are the multi-style step;
/// - **no hard line break** (`'\n'`) — the arm renders one line, not a paragraph;
/// - **default `Start` alignment** — the arm pens from the box left, so Center /
///   End would shift versus parley;
/// - **`Normal` line height** — the arm's baseline matches parley's natural
///   line-box only in `Normal` mode (see [`paint_text_self_hosted`]); a fixed /
///   multiplied line height moves parley's baseline by leading the arm does not
///   model;
/// - **undecorated** — underline / strikethrough are not drawn by the arm;
/// - **not caret-bearing** — an editable [`TextField`](pinion_core::widgets::text_field::TextField) derives its caret /
///   selection / hit-test geometry from a separate parley shaping
///   ([`TextNode::caret_bearing`](pinion_core::scene::TextNode::caret_bearing)),
///   so the arm must not re-shape it (R1072 / R1070.1 caret contract).
///
/// This is necessary, not sufficient: [`paint_text_self_hosted`] additionally
/// falls through to parley when the shaped text would not fit one line (soft
/// wrap). Everything excluded here stays on the parley path.
///
/// R1479 — and the face: the arm holds ONE font, so a leaf whose family (its
/// own, or the process default) is not that font's would be painted in a face
/// nobody asked for. [`SelfHostedTextEngine::serves`] is both halves, asked of
/// the engine that would do the painting.
///
/// R1070 — a thin `TextNode`-shaped convenience over the eligibility SSOT
/// [`crate::text_engine::self_hosted_text_eligible`], so the paint arm and the
/// §5.37 measure arm share one definition of "eligible". Layout / measure picks
/// the §5.37 box only when this same predicate holds, so paint and measure never
/// disagree on which path renders a leaf.
fn self_hosted_eligible(t: &TextNode, engine: &SelfHostedTextEngine) -> bool {
    engine.serves(&t.content, &t.style, &t.runs, t.caret_bearing)
}

/// R1068 §5.37 — paint an [`self_hosted_eligible`] `Scene::Text` leaf through the
/// self-hosted engine: shape with the engine's font, rasterise per glyph into the
/// atlas, and blit each glyph via [`draw_atlased_glyphs`] at the box-relative
/// baseline. Returns `true` when it painted, `false` when the caller must fall
/// through to the parley path — a fall-through is always a safe parley render,
/// never a blank.
///
/// Falls through (returns `false`) when: the size is non-positive; nothing shapes
/// / nothing inks (whitespace); a glyph fails to rasterise; or the shaped advance
/// **exceeds the box width** (`rect.w`), i.e. parley would soft-wrap it to more
/// than one line — the arm renders only one line, so it declines rather than
/// overflow.
///
/// Baseline: `Normal` line height (guaranteed by [`self_hosted_eligible`]) makes the
/// first baseline sit at `ascent + line_gap/2` (half-leading split above) in the
/// `ascent + descent + line_gap` line box. The arm reads that baseline from the
/// SSOT [`LineBoxMetrics`] (shared with the R1070 measure arm's box height), so
/// paint + measure register exactly. Since R1079 that baseline uses the same `OS/2`
/// `USE_TYPO_METRICS` selection parley applies (see [`LineBoxMetrics`]), so it
/// matches parley's `Normal` baseline for the same font (R1068 had pixel-verified
/// the ink to ±2px under the prior hhea-only derivation). Horizontal placement starts
/// at the box left (`Start` alignment, also guaranteed eligible). As of R1070 the
/// measure arm sizes the eligible box to §5.37 too (when wired through
/// [`compute_layout_with_text_measure`](crate::layout::compute_layout_with_text_measure));
/// the single shared decline [`single_line_overflows`] keeps paint and measure from
/// splitting.
fn paint_text_self_hosted(
    out: &mut VelloScene,
    t: &TextNode,
    engine: &SelfHostedTextEngine,
    parent_transform: Affine,
) -> bool {
    let font = engine.font();
    #[allow(
        clippy::cast_precision_loss,
        reason = "font_size_px <= 2^24 px in practice — exact in f32"
    )]
    let px = t.style.font_size_px as f32;
    if px <= 0.0 {
        return false;
    }
    let shaped = shape_paragraph_with_fallback(&[font], &t.content, px);
    if shaped.glyphs.is_empty() {
        return false;
    }
    // Soft-wrap guard (shared SSOT comparison): a bounded box the §5.37 single line
    // would overflow is exactly what parley wraps to multiple lines — decline so
    // parley renders it (the arm is single-line only). `rect.w == 0` is an unmeasured
    // box → `None` → never declines, matching the measure arm's taffy-unbounded probe.
    if single_line_overflows(shaped.advance, (t.rect.w > 0).then_some(t.rect.w)) {
        return false;
    }
    let Ok(rendered) = render_paragraph_atlased(&[font], &shaped, px) else {
        // Rasterisation error (e.g. a not-yet-supported composite glyph) — fall
        // through to the parley path rather than dropping the text.
        return false;
    };
    if rendered.placed.is_empty() {
        // All-whitespace / blank-glyph content — let parley handle it (it also
        // emits nothing, but this keeps the fall-through contract uniform).
        return false;
    }
    // Baseline from the SSOT [`LineBoxMetrics`] shared with the measure arm's box
    // height, so paint + measure register exactly; `else` defers on a malformed
    // font (upem 0).
    let Some(metrics) = LineBoxMetrics::from_font(font, px) else {
        return false;
    };
    let baseline = metrics.baseline_px;
    let transform = node_local_transform(parent_transform, t.rect);
    let needs_clip = matches!(
        t.style.overflow,
        TextOverflow::Clip | TextOverflow::Ellipsis
    );
    if needs_clip {
        let clip_rect = KurboRect::new(
            f64::from(t.rect.x),
            f64::from(t.rect.y),
            f64::from(t.rect.x.saturating_add(t.rect.w)),
            f64::from(t.rect.y.saturating_add(t.rect.h)),
        );
        out.push_clip_layer(Fill::NonZero, parent_transform, &clip_rect);
    }
    // pen origin (0, baseline) in the box-translated frame; glyph pen_y is
    // baseline-relative (0 on the baseline) so the baseline lands at `baseline`.
    draw_atlased_glyphs(out, &rendered, t.style.fg_color, 0.0, baseline, transform);
    if needs_clip {
        out.pop_layer();
    }
    true
}

fn paint_text(
    out: &mut VelloScene,
    t: &TextNode,
    cache: &mut LayoutCache,
    engine: Option<&SelfHostedTextEngine>,
    parent_transform: Affine,
) {
    if t.content.is_empty() {
        return;
    }
    // R1068 §5.37 → production Scene::Text — opt-in self-hosted paint arm. When
    // an engine is supplied AND this leaf is eligible (see `self_hosted_eligible`)
    // AND the self-hosted paint succeeds (it declines, e.g., when the text would
    // soft-wrap), paint through §5.37 and return. Otherwise (no engine, ineligible,
    // or declined) fall through to the unchanged parley path below, so
    // `engine == None` is byte-identical to the pre-R1068 `paint_text`.
    if let Some(engine) = engine
        && self_hosted_eligible(t, engine)
        && paint_text_self_hosted(out, t, engine, parent_transform)
    {
        return;
    }
    // R51.27 §5.37.4 — UAX #9 L4 mirroring is applied inside
    // `LayoutCache::shape` (R51.31 substrate move). paint_adapter
    // passes the raw `TextNode.content` and the cache key (raw text)
    // maps to a Layout whose shape input is the post-mirror string —
    // single LRU lookup covers both the BIDI helper and the parley
    // shape pass, so static labels skip mirror recomputation entirely
    // on steady-state frames.
    let max_width = if t.rect.w > 0 { Some(t.rect.w) } else { None };
    // R713 §5.36 — shape with the styled-run spans so multi-style text
    // paints each run's brush / weight / size. `t.runs.is_empty()` is
    // the single-style fast path (byte-identical to the pre-R713
    // `cache.layout` call). The per-run brush already flows here: the
    // glyph-run loop below reads `run.style().brush` per parley run.
    // R1531 §5.36 — the draw list, not the layout: the walk over parley's runs
    // is a pure function of the shaped layout, and this leaf's is derived once
    // and replayed on every frame that re-encodes it. R1546 moved the binding
    // below the clip so the background bands (a second derivation from the same
    // entry) can take their own borrow of the cache first.
    // R51.188 §5.45 R55.E.1 — compose the inherited transform (e.g.
    // a parent `Scene::Scroll`'s shifted child transform) with the
    // text's own `(t.rect.x, t.rect.y)` translation. Pre-R51.188
    // `paint_text` assumed `Affine::IDENTITY` from the caller; the
    // composition keeps that path bit-identical (IDENTITY * T = T)
    // and lets scroll-embedded text track the scroll offset.
    let transform = node_local_transform(parent_transform, t.rect);
    // R47.6 — Clip + Ellipsis (silent fallback to Clip until R47.x
    // ellipsis pass) wrap the emit in a Vello clip layer keyed to
    // `t.rect`. Visible skips the wrap entirely so a freshly-default
    // TextNode pays no per-frame layer cost.
    let needs_clip = matches!(
        t.style.overflow,
        TextOverflow::Clip | TextOverflow::Ellipsis
    );
    if needs_clip {
        // R51.188 §5.45 R55.E.1 — clip-rect lives in the parent
        // frame (not text-local) because the `transform` above
        // already maps text-local glyph positions into the parent's
        // coordinate space. Passing `parent_transform` lets a
        // scroll-shifted text node clip in its scroll's frame.
        let clip_rect = KurboRect::new(
            f64::from(t.rect.x),
            f64::from(t.rect.y),
            f64::from(t.rect.x.saturating_add(t.rect.w)),
            f64::from(t.rect.y.saturating_add(t.rect.h)),
        );
        out.push_clip_layer(Fill::NonZero, parent_transform, &clip_rect);
    }
    // R1546 §5.36 — the declared backgrounds, filled BEFORE the glyphs so the
    // text reads on top of them (Qt `QTextCharFormat::setBackground`, whose
    // rect its own `QTextLayout::draw` fills first for the same reason). One
    // borrow of the cache at a time: the bands are copied out because
    // `positioned_runs` below needs `&mut cache` again, and a band is 28 bytes
    // against a re-derivation the entry has already paid for.
    let bands: Vec<pinion_text::TextBackground> = cache
        .backgrounds(&t.content, &t.style, &t.runs, max_width)
        .to_vec();
    let runs = cache.positioned_runs(&t.content, &t.style, &t.runs, max_width);
    for band in &bands {
        out.fill(
            Fill::NonZero,
            transform,
            to_peniko(band.color),
            None,
            &KurboRect::new(
                f64::from(band.x),
                f64::from(band.y),
                f64::from(band.x) + f64::from(band.width),
                f64::from(band.y) + f64::from(band.height),
            ),
        );
    }
    for run in runs {
        draw_positioned_run(out, run, transform, to_peniko(run.brush));
        // R47.6 — decoration strokes. parley emits `Some(Decoration)`
        // on a run's `underline / strikethrough` whenever the source
        // TextStyle enabled them (see `LayoutCache::shape`'s
        // `StyleProperty::Underline / Strikethrough` push). R1531 resolves
        // the font-metric fallback and the baseline-relative sign into a
        // `RunDecoration` at derivation time, so this is a stroke.
        paint_decorations(out, run, transform);
    }
    if needs_clip {
        out.pop_layer();
    }
}

/// R47.6 R1531 — emit underline + strikethrough strokes for one
/// [`PositionedRun`]. Each decoration is a horizontal line spanning the run's
/// advance at its resolved y, in its own brush (which parley defaults to the
/// run's foreground).
///
/// The font-metric fallback and the baseline-relative sign flip that used to
/// live here moved into [`pinion_text::glyph_run`] with the rest of the
/// derivation — they are properties of the shaped run, not of the Vello
/// backend, and deriving them here re-ran them on every frame.
fn paint_decorations(out: &mut VelloScene, run: &PositionedRun, transform: Affine) {
    let (start, end) = (f64::from(run.start_x), f64::from(run.end_x));
    // R1540 — the underline goes through the SAME `paint_underline` the cell
    // grid has used since R1399. Until now this function stroked one flat rule
    // for every style, so the tree could draw an undercurl in a terminal and
    // not on screen, with the painter that knew how sitting in this file.
    //
    // `paint_underline` takes the rule's BOTTOM edge and lays the stroke one
    // width above it; parley reports the rule's own y, so the y passed here is
    // that plus one width. Both painters then place the same style identically,
    // which is what lets a GUI and a TUI screenshot of one document agree.
    if let Some(ul) = run.underline.as_ref() {
        paint_underline(
            out,
            transform,
            to_peniko(ul.rule.brush),
            start,
            end,
            f64::from(ul.rule.y) + f64::from(ul.rule.size),
            f64::from(ul.rule.size),
            ul.style,
        );
    }
    if let Some(deco) = run.strikethrough.as_ref() {
        // A strikethrough has one form in both SGR (9) and Qt, so it stays on
        // the plain primitive rather than borrowing a form axis it cannot use.
        stroke_hrule(
            out,
            transform,
            to_peniko(deco.brush),
            start,
            end,
            f64::from(deco.y),
            f64::from(deco.size),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_profile::DrawProfileNode;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, SceneNodeKind, TextNode};
    use pinion_core::style::{BoxStyle, Color, TextStyle};
    use std::cell::Cell;

    #[test]
    fn to_peniko_preserves_all_channels_including_alpha() {
        // R46.3.1 invariant: the conversion is loss-less across all
        // four channels. R46.3 inline had alpha hardcoded to 255; the
        // framework primitive fixes that — pinion::Color::rgba(_,_,_,a)
        // round-trips to peniko via from_rgba8 verbatim.
        let pinion = Color::rgba(0x12, 0x34, 0x56, 0x78);
        let peniko = to_peniko(pinion);
        assert_eq!(peniko, PenikoColor::from_rgba8(0x12, 0x34, 0x56, 0x78));
    }

    #[test]
    fn to_peniko_alpha_zero_is_transparent() {
        // The legacy softbuffer `0x00RRGGBB` literal decodes through
        // Color::from_argb to alpha = 0 = transparent. Callers that
        // expected opacity from `0x00FF_3366` must migrate to
        // Color::rgb(0xFF, 0x33, 0x66); the framework no longer masks
        // the bug by hardcoding 255.
        let from_argb = Color::from_argb(0x00ff_3366);
        let peniko = to_peniko(from_argb);
        assert_eq!(peniko, PenikoColor::from_rgba8(0xff, 0x33, 0x66, 0x00));
    }

    // R1063 §5.37 → §5.16 — Coverage AA mask → peniko::ImageData seam.

    #[test]
    fn coverage_to_image_data_empty_is_none() {
        // A space / control glyph rasterises to an empty mask; the seam emits
        // no image (the draw_coverage no-op contract).
        assert!(coverage_to_image_data(&Coverage::empty(), Color::rgb(0, 0, 0)).is_none());
    }

    #[test]
    fn coverage_to_image_data_maps_alpha_per_pixel_and_constant_color() {
        // 2×1 mask: a fully-transparent pixel then a fully-inked one. The
        // result is straight-alpha RGBA8 carrying the brush RGB at every pixel
        // and the mask value as alpha — what Vello's image path src-overs.
        let cov = Coverage {
            width: 2,
            height: 1,
            left: 0,
            top: 0,
            alpha: vec![0, 255],
        };
        let img = coverage_to_image_data(&cov, Color::rgb(10, 20, 30)).expect("non-empty");
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.format, ImageFormat::Rgba8);
        assert_eq!(img.alpha_type, ImageAlphaType::Alpha);
        assert_eq!(img.data.data(), &[10, 20, 30, 0, 10, 20, 30, 255]);
    }

    #[test]
    fn coverage_to_image_data_modulates_mask_by_brush_alpha() {
        // A translucent brush dims the whole run: out_a = mask * color.a / 255.
        // Full coverage (255) under a half-alpha brush (128) -> 128.
        let cov = Coverage {
            width: 1,
            height: 1,
            left: 0,
            top: 0,
            alpha: vec![255],
        };
        let img = coverage_to_image_data(&cov, Color::rgba(0, 0, 0, 128)).expect("non-empty");
        assert_eq!(img.data.data(), &[0, 0, 0, 128]);
    }

    #[test]
    fn coverage_to_image_data_rounds_alpha_modulation_to_nearest() {
        // mask 200 x brush-alpha 200 = 40000; 40000/255 = 156.86. Round-to-nearest
        // is 157 (floor would give 156). Pins the §5.37 house convention
        // (shape::over / raster quantization both round) so a regression to a
        // bare floor — a ≤1-LSB darkening of every AA run — fails here.
        let cov = Coverage {
            width: 1,
            height: 1,
            left: 0,
            top: 0,
            alpha: vec![200],
        };
        let img = coverage_to_image_data(&cov, Color::rgba(0, 0, 0, 200)).expect("non-empty");
        assert_eq!(img.data.data(), &[0, 0, 0, 157]);
    }

    #[test]
    fn draw_coverage_empty_mask_emits_nothing() {
        // The no-op contract holds at the draw site: an empty mask must not
        // panic and must leave the scene untouched (encoded op count unchanged).
        let mut scene = VelloScene::new();
        draw_coverage(
            &mut scene,
            &Coverage::empty(),
            Color::rgb(0, 0, 0),
            4.0,
            8.0,
            Affine::IDENTITY,
        );
        // A fresh scene that received only a no-op draw encodes no draw tags.
        assert_eq!(scene.encoding().n_paths, 0);
    }

    #[test]
    fn draw_coverage_nonempty_mask_emits_one_fill() {
        // A non-empty mask blits through Vello's image path (draw_image ->
        // fill), so exactly one path is encoded. Proves the seam wires to the
        // scene without needing a GPU (the end-to-end pixel check is the
        // realgpu forcing consumer in pinion-shell).
        let cov = Coverage {
            width: 3,
            height: 2,
            left: -1,
            top: -5,
            alpha: vec![255; 6],
        };
        let mut scene = VelloScene::new();
        draw_coverage(
            &mut scene,
            &cov,
            Color::rgb(255, 0, 0),
            10.0,
            20.0,
            Affine::IDENTITY,
        );
        assert_eq!(scene.encoding().n_paths, 1);
    }

    // R1065 §5.37.9 → §5.16 — per-glyph GlyphAtlas → peniko::ImageData + the
    // per-glyph-quad paint path (the production-direction successor to the
    // whole-paragraph draw_coverage blit).

    /// `NotoSans` (Latin) — the §5.37.1 parser fixture, reused to drive the shaper.
    /// The §5.37 integer-snap invariant makes rasterisation reproducible, so these
    /// are ZERO-FLAKE. Production font policy for §5.37 is a separate, later
    /// decision; this is a dev forcing consumer.
    const NOTO_FIXTURE: &[u8] =
        include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");

    /// Shape + atlas-place `"Hi"` at 32 px through the real §5.37 engine.
    fn rendered_hi() -> pinion_text_font::RenderedGlyphs {
        let font = pinion_text_font::Font::from_bytes(NOTO_FIXTURE.to_vec())
            .expect("parse NotoSans fixture");
        let shaped = pinion_text_font::shape_paragraph(&font, "Hi", 32.0);
        pinion_text_font::render_paragraph_atlased(&[&font], &shaped, 32.0)
            .expect("atlas-render Hi")
    }

    #[test]
    fn atlas_to_image_data_empty_atlas_is_none() {
        // An atlas with nothing packed (no glyph rasterised) has no pixels, so the
        // seam emits no image — the draw_atlased_glyphs no-op contract.
        let atlas = GlyphAtlas::new(64);
        assert!(atlas_to_image_data(&atlas, Color::rgb(0, 0, 0)).is_none());
    }

    #[test]
    fn atlas_to_image_data_tints_packed_atlas() {
        // The packed atlas bitmap becomes one tinted image of matching dimensions:
        // every pixel carries the brush RGB and the atlas coverage as alpha —
        // inked where a glyph packed, transparent in the gaps. A width/height swap
        // (wrong GPU row stride) is caught by the explicit dimension asserts; the
        // linear per-pixel check alone would not see it.
        let rendered = rendered_hi();
        let atlas = &rendered.atlases[0];
        let img = atlas_to_image_data(atlas, Color::rgb(7, 8, 9)).expect("packed atlas");
        assert_eq!(usize::try_from(img.width).unwrap(), atlas.width());
        assert_eq!(usize::try_from(img.height).unwrap(), atlas.height());
        let px = img.data.data();
        assert_eq!(px.len(), atlas.alpha().len() * 4);
        for (i, &cov) in atlas.alpha().iter().enumerate() {
            assert_eq!(&px[i * 4..i * 4 + 4], &[7, 8, 9, cov], "pixel {i}");
        }
        // Non-vacuous: the atlas carries both ink and (right-of-shelf) blank pixels.
        assert!(atlas.alpha().iter().any(|&a| a > 0), "atlas has no ink");
        assert!(atlas.alpha().contains(&0), "atlas has no blank pixel");
    }

    #[test]
    fn draw_atlased_glyphs_empty_emits_nothing() {
        // No placements -> no fills; must not panic, scene untouched.
        let mut scene = VelloScene::new();
        draw_atlased_glyphs(
            &mut scene,
            &pinion_text_font::RenderedGlyphs::default(),
            Color::rgb(0, 0, 0),
            4.0,
            8.0,
            Affine::IDENTITY,
        );
        assert_eq!(scene.encoding().n_paths, 0);
    }

    #[test]
    fn draw_atlased_glyphs_emits_one_fill_per_glyph() {
        // The defining contrast with draw_coverage: that blits ONE whole-paragraph
        // path; this draws one quad PER glyph. "Hi" is two inked glyphs, so the
        // scene encodes exactly two fills — proof the atlas is kept per-glyph, not
        // flattened into a single mask.
        let rendered = rendered_hi();
        assert_eq!(rendered.placed.len(), 2, "Hi should shape to two glyphs");
        let mut scene = VelloScene::new();
        draw_atlased_glyphs(
            &mut scene,
            &rendered,
            Color::rgb(255, 0, 0),
            10.0,
            20.0,
            Affine::IDENTITY,
        );
        assert_eq!(
            scene.encoding().n_paths,
            u32::try_from(rendered.placed.len()).unwrap()
        );
    }

    #[test]
    fn draw_atlased_glyphs_styled_one_fill_per_colored_glyph() {
        // Per-glyph colours still emit one quad per glyph. "Hi" in two colours ->
        // two fills (the colour lives in the brush, invisible to n_paths; the pixel
        // proof that each glyph took its own colour is the realgpu seam test).
        let rendered = rendered_hi();
        let colors = [Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)];
        let mut scene = VelloScene::new();
        draw_atlased_glyphs_styled(&mut scene, &rendered, &colors, 10.0, 20.0, Affine::IDENTITY);
        assert_eq!(
            scene.encoding().n_paths,
            u32::try_from(rendered.placed.len()).unwrap()
        );
    }

    #[test]
    fn draw_atlased_glyphs_styled_skips_uncolored_tail() {
        // Fewer colours than glyphs: the uncoloured tail is dropped (defensive), so
        // only the coloured prefix paints. "Hi" with one colour -> one fill.
        let rendered = rendered_hi();
        let colors = [Color::rgb(255, 0, 0)];
        let mut scene = VelloScene::new();
        draw_atlased_glyphs_styled(&mut scene, &rendered, &colors, 10.0, 20.0, Affine::IDENTITY);
        assert_eq!(scene.encoding().n_paths, 1);
    }

    #[test]
    fn root_background_extracts_root_container_fill() {
        let scene = Scene::Container(
            ContainerNode::new(vec![]).with_style(BoxStyle::filled(Color::rgb(0xff, 0, 0))),
        );
        let bg = root_background(&scene);
        assert_eq!(bg, PenikoColor::from_rgba8(0xff, 0, 0, 0xff));
    }

    #[test]
    fn root_background_falls_back_to_black_for_non_container() {
        // Any non-Container root (Box, External, ...) returns BLACK —
        // there's no canonical "scene background" without a Container.
        let scene = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 10, 10),
            Color::rgb(0xff, 0, 0),
        ));
        let bg = root_background(&scene);
        assert_eq!(bg, PenikoColor::BLACK);
    }

    #[test]
    fn to_vello_walks_container_and_box_children() {
        // The walker reaches every BoxNode under a Container. Verify
        // by Cell-counting hook hits (Fn bound; interior mutability
        // for test-side state).
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(
                BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgb(0xff, 0, 0)).with_tag("a"),
            ),
            Scene::Box(
                BoxNode::filled(Rect::new(20, 0, 10, 10), Color::rgb(0, 0xff, 0)).with_tag("b"),
            ),
        ]));
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        let hits = Cell::new(0_u32);
        to_vello(
            &scene,
            &|_b: &BoxNode| {
                hits.set(hits.get() + 1);
                None
            },
            &mut cache,
            &mut vello,
        );
        assert_eq!(hits.get(), 2, "hook called once per BoxNode");
    }

    #[test]
    fn to_vello_hook_some_overrides_box_native_fill() {
        // When the hook returns Some, that color replaces the box's
        // `style.fill`. We can't read back the emitted Vello commands
        // from outside the crate, but we can verify the hook was
        // consulted with the correct BoxNode (tag-driven dispatch
        // matches ai-introspect-demo's info_panel pattern).
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(
                BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgb(0xff, 0, 0))
                    .with_tag("info_panel"),
            ),
            Scene::Box(
                BoxNode::filled(Rect::new(20, 0, 10, 10), Color::rgb(0, 0xff, 0))
                    .with_tag("save_btn"),
            ),
        ]));
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        let overrides = Cell::new(0_u32);
        let passthroughs = Cell::new(0_u32);
        to_vello(
            &scene,
            &|b: &BoxNode| {
                if b.tag.as_deref() == Some("info_panel") {
                    overrides.set(overrides.get() + 1);
                    Some(Color::rgb(0, 0, 0xff))
                } else {
                    passthroughs.set(passthroughs.get() + 1);
                    None
                }
            },
            &mut cache,
            &mut vello,
        );
        assert_eq!(overrides.get(), 1);
        assert_eq!(passthroughs.get(), 1);
    }

    /// R1511 §5.16 §2 #6 — a `Scene::Container` paints the
    /// [`BoxStyle::border`](pinion_core::style::BoxStyle::border) it declares,
    /// exactly as a `Scene::Box` carrying the same rect and style does.
    ///
    /// Both node types hang the same sidecar `BoxStyle` off the same `rect`,
    /// and the two other backends already read it that way: the TUI walker
    /// (`pinion_tui::paint::paint_container`) draws a container's border as
    /// box-drawing cells, and the PDF projector (`pinion_pdf`) routes
    /// `Scene::Container` through the same `paint_box` that strokes it. Only
    /// the vello adapter used to stroke a border in the `Scene::Box` arm
    /// alone, so a container's declaration reached two of the three renderers
    /// — the divergence §2 #6 exists to forbid. The TUI walker's own doc
    /// asserted the parity ("the order matches the Vello paint adapter ... so
    /// the visual stack is identical across the two backends") that did not
    /// hold.
    ///
    /// The assertion is byte equality of the geometry streams rather than a
    /// count: a stroke that lands with different geometry on the two node
    /// types is the same class of defect as one that never lands, and the
    /// count alone cannot see it. No GPU, so it gates every ordinary `cargo
    /// test`; the pixel witness for the same claim is
    /// `r1511_container_border_reaches_pixels` in
    /// `pinion_shell::headless_screenshot`.
    ///
    /// BOTH walkers are measured, and that is load-bearing rather than
    /// thorough: the first draft asserted only `to_vello` and passed while the
    /// CACHED walker still dropped the stroke, because a cacheable container
    /// at identity transform takes a fast path that open-coded the
    /// shadows+fill sequence a THIRD time. The pixel guard is what caught it.
    /// An entry point a guard does not enter is an arm the guard cannot see.
    #[test]
    fn r1511_container_paints_the_border_it_declares() {
        use crate::image_cache::ImageCache;
        use pinion_core::style::{Border, BoxStyle};
        let rect = Rect::new(12, 8, 120, 64);
        let style = BoxStyle::filled(Color::rgb(0x20, 0x20, 0x20))
            .with_border(Border::new(Color::rgb(0xff, 0, 0), 3));

        let encode = |scene: &Scene| -> (Vec<u32>, u32, u32) {
            let mut vello = VelloScene::new();
            let mut cache = LayoutCache::new();
            to_vello(scene, &|_: &BoxNode| None, &mut cache, &mut vello);
            let mut cached = VelloScene::new();
            let mut text_cache = LayoutCache::new();
            let mut image_cache = ImageCache::new();
            let mut fragments = FragmentCache::new();
            to_vello_cached(
                scene,
                &|_: &BoxNode| None,
                &mut text_cache,
                &mut image_cache,
                &mut fragments,
                &mut cached,
            );
            assert_eq!(
                (
                    cached.encoding().n_paths,
                    cached.encoding().n_path_segments,
                    cached.encoding().path_data.clone(),
                ),
                (
                    vello.encoding().n_paths,
                    vello.encoding().n_path_segments,
                    vello.encoding().path_data.clone(),
                ),
                "the cached walker must encode what the direct one does"
            );
            let enc = vello.encoding();
            (enc.path_data.clone(), enc.n_paths, enc.n_path_segments)
        };

        let boxed = Scene::Box(BoxNode::new(rect, style.clone()));
        let mut node = ContainerNode::new(vec![]).with_style(style.clone());
        node.rect = rect;
        let contained = Scene::Container(node);

        let (box_data, box_paths, box_segs) = encode(&boxed);
        let (c_data, c_paths, c_segs) = encode(&contained);
        assert_eq!(
            (c_paths, c_segs),
            (box_paths, box_segs),
            "a childless Container with a bordered style must encode the same \
             paths as the equivalent Box; the border stroke is the difference"
        );
        assert_eq!(
            c_data, box_data,
            "the container's border must land with the SAME geometry as the \
             box's, not merely the same path count"
        );

        // The container strokes its border BEFORE recursing, so a child's
        // geometry can only ever be APPENDED to what the childless container
        // encodes. Prefix equality states that order without reaching into
        // vello's stream layout: paint the border after the children and the
        // child's segments land first, breaking the prefix.
        let child = Scene::Box(BoxNode::new(
            Rect::new(rect.x + 20, rect.y + 20, 24, 16),
            BoxStyle::filled(Color::rgb(0, 0x80, 0)),
        ));
        let mut parent = ContainerNode::new(vec![child]).with_style(style.clone());
        parent.rect = rect;
        let (with_child, child_paths, _) = encode(&Scene::Container(parent));
        assert!(
            with_child.starts_with(&c_data),
            "the container's own decoration must be encoded before its \
             children ({} words with a child vs {} without)",
            with_child.len(),
            c_data.len()
        );
        assert!(
            child_paths > c_paths,
            "the child contributes its own path on top"
        );

        // Non-vacuous in both directions: dropping the border from BOTH node
        // types must move the shared encoding, so the equality above is
        // asserting the presence of a stroke and not comparing two empties.
        let bare = BoxStyle::filled(Color::rgb(0x20, 0x20, 0x20));
        let mut bare_node = ContainerNode::new(vec![]).with_style(bare.clone());
        bare_node.rect = rect;
        let (bare_data, bare_paths, _) = encode(&Scene::Container(bare_node));
        assert!(
            c_paths > bare_paths && c_data != bare_data,
            "a bordered container must encode more than an unbordered one \
             (bordered {c_paths} paths, bare {bare_paths})"
        );
    }

    #[test]
    fn stroke_rect_inside_placement_inset_matches_softbuffer_geometry() {
        // R46.3.2 — the default Border placement (Inside) must inset
        // the centred stroke by width/2 so the entire stroke lies
        // within the rect. We can't read back Vello's emitted draw
        // commands; instead we verify the placement field plumbs
        // through stroke_rect by ensuring no panic on each variant.
        use pinion_core::style::{Border, BorderPlacement, BoxStyle};
        for placement in [
            BorderPlacement::Inside,
            BorderPlacement::Center,
            BorderPlacement::Outside,
        ] {
            let border = Border::new(Color::rgb(0xff, 0, 0), 4).with_placement(placement);
            let style = BoxStyle::filled(Color::TRANSPARENT).with_border(border);
            let scene = Scene::Box(BoxNode::new(Rect::new(10, 10, 100, 100), style));
            let mut vello = VelloScene::new();
            let mut cache = LayoutCache::new();
            to_vello(&scene, &|_| None, &mut cache, &mut vello);
        }
    }

    #[test]
    fn to_vello_nested_container_recurses() {
        // Two-level Container nesting: outer + inner + leaf box. The
        // walker must visit the leaf box's hook.
        let inner = ContainerNode::new(vec![Scene::Box(
            BoxNode::filled(Rect::new(10, 10, 10, 10), Color::rgb(0, 0xff, 0)).with_tag("leaf"),
        )]);
        let outer = ContainerNode::new(vec![Scene::Container(inner)]);
        let scene = Scene::Container(outer);
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        let saw_leaf = Cell::new(false);
        to_vello(
            &scene,
            &|b: &BoxNode| {
                if b.tag.as_deref() == Some("leaf") {
                    saw_leaf.set(true);
                }
                None
            },
            &mut cache,
            &mut vello,
        );
        assert!(saw_leaf.get(), "nested leaf BoxNode must be visited");
    }

    #[test]
    fn to_vello_text_arm_populates_cache() {
        // R47.3 §5.36 — Scene::Text walks via paint_text which calls
        // LayoutCache::layout; the cache should hold the entry after
        // one walk, and a second walk over the same text should not
        // grow the cache (steady-state cache hit).
        let scene = Scene::Text(TextNode::styled(
            "Hello",
            Rect::new(0, 0, 200, 32),
            TextStyle::new().with_size_px(16),
        ));
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "first paint populates cache");
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "repeat paint hits cache, no growth");
    }

    #[test]
    fn to_vello_text_arm_skips_empty_content() {
        // Empty `t.content` short-circuits before the cache is touched —
        // parley would produce an empty layout but the walk has no
        // glyphs to emit, so skipping early avoids the wasted shaping
        // work.
        let scene = Scene::Text(TextNode::styled(
            "",
            Rect::new(0, 0, 200, 32),
            TextStyle::new().with_size_px(16),
        ));
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 0, "empty content does not populate cache");
    }

    #[test]
    fn to_vello_text_arm_decoration_no_panic() {
        // R47.6 §5.36 — decoration wire emits parley StyleProperty::
        // Underline + Strikethrough; paint_text walks parley's
        // `style().underline / strikethrough` and strokes a horizontal
        // line per decoration. Cannot inspect Vello's emitted draw
        // commands from outside the crate; assert no panic on every
        // combination instead.
        use pinion_core::style::TextDecoration;
        for deco in [
            TextDecoration::none(),
            TextDecoration::underline(),
            TextDecoration::strikethrough(),
            TextDecoration::both(),
        ] {
            let scene = Scene::Text(TextNode::styled(
                "Hi",
                Rect::new(0, 0, 200, 32),
                TextStyle::new().with_size_px(16).with_decoration(deco),
            ));
            let mut vello = VelloScene::new();
            let mut cache = LayoutCache::new();
            to_vello(&scene, &|_| None, &mut cache, &mut vello);
        }
    }

    #[test]
    fn to_vello_text_arm_overflow_clip_pushes_layer_safely() {
        // R47.6 — TextOverflow::Clip wraps paint_text in
        // push_clip_layer / pop_layer. The wrap must balance (every
        // push matched by a pop) so the Vello scene encoding stays
        // valid.
        //
        // R1556 — the original body stopped at "the walk did not panic", under
        // a comment stating we "cannot read the encoded layer stack from
        // outside the crate". That was never true: `vello::Scene::encoding()`
        // is public, and `draw_work_of` reads it. So the balance is now
        // ASSERTED rather than inferred from the absence of a panic — and the
        // clip is asserted to have been pushed at all, which no-panic could not
        // distinguish from the arm silently not clipping.
        //
        // `Ellipsis` clips too, and that is the documented behaviour rather
        // than an accident: parley exposes no line-truncation API, so the arm
        // falls back to `Clip` (see `paint_text`). This test asserted `0` there
        // on the first draft — a guess, corrected by running it, and now the
        // only place that fallback is stated as a checkable fact instead of a
        // sentence in a doc comment.
        use pinion_core::style::TextOverflow;
        for (overflow, want_layers) in [
            (TextOverflow::Visible, 0),
            (TextOverflow::Clip, 1),
            (TextOverflow::Ellipsis, 1),
        ] {
            let scene = Scene::Text(TextNode::styled(
                "OverflowingContent",
                Rect::new(0, 0, 50, 16), // intentionally tight
                TextStyle::new().with_size_px(16).with_overflow(overflow),
            ));
            let mut vello = VelloScene::new();
            let mut cache = LayoutCache::new();
            to_vello(&scene, &|_| None, &mut cache, &mut vello);
            assert_eq!(
                draw_work_of(&vello).layers,
                want_layers,
                "{overflow:?} pushed the wrong number of clip layers",
            );
            assert_eq!(
                vello.encoding().n_open_clips,
                0,
                "{overflow:?} left a clip layer open",
            );
        }
    }

    // ----- R1556 §5.16 — the frame's DRAW census -----

    #[test]
    fn r1556_a_layer_costs_two_clip_entries() {
        // Pins the one quantity `draw_work_of` MODELS rather than reads: the
        // encoder's `n_clips` counts a begin and an end separately, so a
        // balanced layer costs `CLIP_ENTRIES_PER_LAYER`. That is a fact about a
        // crate this project does not own, and R1550 recorded exactly this
        // shape as the class that goes wrong SILENTLY — the arithmetic keeps
        // compiling and the number keeps looking plausible. This is what makes
        // it loud.
        for pushes in 0_u32..4 {
            let mut vello = VelloScene::new();
            for _ in 0..pushes {
                vello.push_clip_layer(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    &vello::kurbo::Rect::new(0.0, 0.0, 10.0, 10.0),
                );
            }
            for _ in 0..pushes {
                vello.pop_layer();
            }
            assert_eq!(
                vello.encoding().n_clips,
                pushes * CLIP_ENTRIES_PER_LAYER,
                "vello changed how a balanced clip layer is counted",
            );
            assert_eq!(draw_work_of(&vello).layers, pushes);
        }

        // …and the derivation is exact for an UNBALANCED encode too, which is
        // the case a bare `n_clips / 2` truncates away. Three pushed, two
        // popped: the frame pushed three layers and must report three.
        let mut open = VelloScene::new();
        for _ in 0..3 {
            open.push_clip_layer(
                Fill::NonZero,
                Affine::IDENTITY,
                &vello::kurbo::Rect::new(0.0, 0.0, 10.0, 10.0),
            );
        }
        open.pop_layer();
        open.pop_layer();
        assert_eq!(open.encoding().n_open_clips, 1, "one layer left open");
        assert_eq!(
            draw_work_of(&open).layers,
            3,
            "an open layer is still a layer the frame pushed",
        );
    }

    #[test]
    fn r1556_two_scenes_of_equal_node_count_draw_unequal_work() {
        // The whole reason this census exists, at the smallest scale that can
        // state it. Two scenes with the SAME number of nodes — one Text leaf
        // each — hand the renderer two orders of magnitude of different work.
        // R1538's node census reports both as identical, which is correct and
        // is precisely its limit.
        let short = text_scene("ok");
        let long = text_scene(&"lorem ipsum dolor sit amet ".repeat(20));

        let (a, b) = (encode_uncached(&short), encode_uncached(&long));
        assert_eq!(
            scene_nodes_of(&short),
            scene_nodes_of(&long),
            "the premise: the node census cannot tell these apart",
        );
        assert_eq!((a.glyphs, b.glyphs), (2, 540));
        assert!(
            b.glyphs > a.glyphs * 50,
            "the draw census must: {} vs {} glyphs",
            a.glyphs,
            b.glyphs,
        );

        // …and the text lands in the glyph fields and NOWHERE else. Pinned
        // because it is the meaning of `path_segments`, and it was not the
        // meaning this test first assumed: `vello` encodes a run as positioned
        // glyphs and resolves their outlines to paths downstream of the
        // encoding, so a pure-text frame encodes ZERO paths. The two axes are
        // disjoint, which is what lets a frame's text cost and its vector cost
        // be read apart instead of summed into one uninterpretable number.
        assert_eq!(
            (a.paths, a.path_segments, b.paths, b.path_segments),
            (0, 0, 0, 0),
            "text is not encoded geometry",
        );
        assert_eq!(
            (a.draws, b.draws),
            (a.glyph_runs, b.glyph_runs),
            "one draw command per shaped run, and nothing else drawn here",
        );
    }

    #[test]
    fn r1556_a_replayed_fragment_is_counted_like_an_encoded_one() {
        // The property that makes this a census of the FRAME rather than of the
        // walk: paint the same scene twice through one FragmentCache. The
        // second paint is served by the cache — it walks far less — and the GPU
        // is handed exactly the same drawing either way, so the two censuses
        // must be equal. A tally kept by the walker would report the second
        // frame as drawing almost nothing, which is the reading a profiler must
        // never produce.
        let scene = Scene::Container(
            ContainerNode::new(vec![
                text_scene("first row"),
                text_scene("second row"),
                Scene::Box(BoxNode::filled(
                    Rect::new(0, 0, 40, 40),
                    Color::rgb(0x20, 0x30, 0x40),
                )),
            ])
            .with_style(BoxStyle::filled(Color::rgb(0xf0, 0xf0, 0xf0))),
        );
        let mut text_cache = LayoutCache::new();
        let mut image_cache = ImageCache::new();
        let mut fragments = FragmentCache::new();

        let mut first = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut fragments,
            &mut first,
        );
        let walked_first = fragments.nodes_walked_last_paint();

        let mut second = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut fragments,
            &mut second,
        );
        let walked_second = fragments.nodes_walked_last_paint();

        assert!(
            walked_second < walked_first,
            "premise: the second paint must hit the cache ({walked_first} -> {walked_second})",
        );
        assert_eq!(
            draw_work_of(&first),
            draw_work_of(&second),
            "a replayed fragment draws the same work it drew when encoded",
        );
        assert!(
            draw_work_of(&second).glyphs > 0,
            "…and that work is not zero",
        );
    }

    /// The scene the R1557 profile tests attribute: a styled root holding two
    /// text leaves and a filled box, so glyphs, paths and the container's own
    /// decoration are all non-zero and all distinguishable.
    fn profile_scene() -> Scene {
        let mut root = ContainerNode::new(vec![
            Scene::Text(
                TextNode::styled(
                    "first row",
                    Rect::new(0, 0, 4000, 40),
                    TextStyle::new().with_size_px(16),
                )
                .with_tag("first"),
            ),
            text_scene("second row"),
            Scene::Box(BoxNode::filled(
                Rect::new(0, 40, 40, 40),
                Color::rgb(0x20, 0x30, 0x40),
            )),
        ])
        .with_style(BoxStyle::filled(Color::rgb(0xf0, 0xf0, 0xf0)));
        // The rect is set explicitly because this fixture is never laid out,
        // and a zero-area container paints no background — which would make
        // "a container's own work is its decoration" vacuously true below.
        root.rect = Rect::new(0, 0, 4000, 120);
        Scene::Container(root)
    }

    /// (R1558) A root holding a leaf and a nested panel, the panel itself
    /// holding a leaf and a second container — the shallowest scene in which
    /// "profile that subtree alone" is a different walk from "profile
    /// everything", at three levels so a scoped profile has descendants of its
    /// own to get wrong.
    ///
    /// Every leaf carries text so the comparison covers the term a node census
    /// cannot reach, and the containers are styled so their `own` is non-zero
    /// (an all-zero subtree compares equal to any other all-zero subtree).
    fn nested_profile_scene() -> Scene {
        let mut inner = ContainerNode::new(vec![
            text_scene("inner row"),
            Scene::Box(BoxNode::filled(
                Rect::new(0, 90, 30, 20),
                Color::rgb(0x80, 0x10, 0x10),
            )),
        ])
        .with_style(BoxStyle::filled(Color::rgb(0xcc, 0xcc, 0xcc)));
        inner.rect = Rect::new(0, 80, 400, 40);
        inner.tag = Some("inner".into());

        let mut panel = ContainerNode::new(vec![text_scene("panel row"), Scene::Container(inner)])
            .with_style(BoxStyle::filled(Color::rgb(0xdd, 0xdd, 0xdd)));
        panel.rect = Rect::new(0, 40, 400, 120);
        panel.tag = Some("panel".into());

        let mut root = ContainerNode::new(vec![text_scene("root row"), Scene::Container(panel)])
            .with_style(BoxStyle::filled(Color::rgb(0xf0, 0xf0, 0xf0)));
        root.rect = Rect::new(0, 0, 400, 200);
        Scene::Container(root)
    }

    /// (R1558) The panel subtree of [`nested_profile_scene`], on its own.
    fn nested_panel() -> Scene {
        let Scene::Container(root) = nested_profile_scene() else {
            unreachable!("the fixture's root is a Container")
        };
        root.children
            .into_iter()
            .nth(1)
            .expect("the fixture's root has a second child")
    }

    /// (R1558) Assert two profile subtrees are the same attribution — every
    /// node, every field, recursively.
    ///
    /// The root's own `segment` is exempt and only the root's: a segment says
    /// where a node sits among its PARENT's children, so it is precisely what
    /// must differ between a node profiled in place and the same node profiled
    /// as a root of its own. Every descendant's segment is compared, because
    /// a scoped profile whose interior addressing shifted would still satisfy
    /// a comparison of the counts alone.
    fn assert_same_attribution(alone: &DrawProfileNode, in_place: &DrawProfileNode, at: &str) {
        assert_eq!(alone.kind, in_place.kind, "kind at {at}");
        assert_eq!(alone.tag, in_place.tag, "tag at {at}");
        assert_eq!(alone.total, in_place.total, "total at {at}");
        assert_eq!(alone.own, in_place.own, "own at {at}");
        assert_eq!(
            alone.children.len(),
            in_place.children.len(),
            "child count at {at}"
        );
        for (i, (a, b)) in alone
            .children
            .iter()
            .zip(in_place.children.iter())
            .enumerate()
        {
            let at = format!("{at}/{i}");
            assert_eq!(a.segment, b.segment, "segment at {at}");
            assert_same_attribution(a, b, &at);
        }
    }

    /// Profile one paint of `scene` into a cold fragment cache — the shape
    /// `ShellCore::draw_profile_for_window` uses — and hand back the profile
    /// beside the census of the scene that was actually encoded.
    fn profile_once(scene: &Scene) -> (DrawProfileNode, DrawWork) {
        let mut text_cache = LayoutCache::new();
        let mut image_cache = ImageCache::new();
        let mut fragments = FragmentCache::new();
        let mut out = VelloScene::new();
        let mut profiler = DrawProfiler::new();
        to_vello_cached_profiled(
            scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut fragments,
            None,
            &mut out,
            true,
            true,
            &mut profiler,
        );
        let root = profiler
            .finish(Vec::new())
            .root
            .expect("a painted scene has a root");
        (root, draw_work_of(&out))
    }

    #[test]
    fn r1557_the_profile_partitions_the_frame_it_measured() {
        let (root, encoded) = profile_once(&profile_scene());
        // The root's inclusive total IS the frame's census — the profile is
        // measuring the same artifact `scene/frame_timings` reports, not a
        // parallel accounting that could drift from it.
        assert_eq!(
            root.total, encoded,
            "the root's inclusive total is the whole encoded scene",
        );
        // …and the exclusive costs partition it. This is the identity that
        // fails if a saturating subtraction ever clamps, if an arm opens a
        // profile frame without closing it, or if work lands outside the span
        // being measured.
        assert_eq!(
            root.own_sum(),
            root.total,
            "every node's own work sums to the root's total",
        );
        assert_eq!(root.node_count(), 4, "root + two text leaves + one box");
        assert!(encoded.glyphs > 0 && encoded.paths > 0, "premise: real ink");
    }

    #[test]
    fn r1557_text_is_attributed_to_the_leaf_that_drew_it() {
        // The claim no Qt surface makes: which item the glyphs belong to. A
        // `Text` leaf is one node whether it holds two glyphs or four thousand,
        // so this is the term a node census cannot reach.
        let (root, _) = profile_once(&profile_scene());
        assert_eq!(
            root.own.glyphs, 0,
            "the container drew a background, not glyphs",
        );
        assert!(root.own.paths > 0, "…and the background is a path");
        let leaf_glyphs: u32 = root
            .children
            .iter()
            .filter(|c| c.kind == SceneNodeKind::Text)
            .map(|c| c.own.glyphs)
            .sum();
        assert_eq!(
            leaf_glyphs, root.total.glyphs,
            "every glyph in the frame belongs to a text leaf",
        );
        for child in &root.children {
            assert!(
                child.children.is_empty(),
                "a leaf has no children to attribute to",
            );
            assert_eq!(child.own, child.total, "a leaf's own work is all of it");
        }
        // Glyph outlines never reach `path_segments` (they are resolved
        // downstream of the encoding), so the text leaves contribute geometry
        // to neither field but their own.
        let text_leaf = root
            .children
            .iter()
            .find(|c| c.tag.as_deref() == Some("first"))
            .expect("the tagged text leaf");
        assert!(text_leaf.own.glyphs > 0);
        assert_eq!(text_leaf.own.paths, 0, "text encodes no paths");
    }

    #[test]
    fn r1557_a_row_is_addressed_the_way_every_other_method_addresses_it() {
        let (root, _) = profile_once(&profile_scene());
        assert_eq!(root.segment, None, "the root consumes no path segment");
        assert_eq!(
            root.children[0].segment.as_deref(),
            Some("first"),
            "a tagged child is addressed by its tag",
        );
        assert_eq!(
            root.children[1].segment.as_deref(),
            Some("1"),
            "an untagged child by its index",
        );
        assert_eq!(root.children[2].segment.as_deref(), Some("2"));
        // The rule is not re-implemented here: it is
        // `Scene::path_segment_at`, which is also what `Scene::hit_test` puts
        // in a `HitPath`. So a profile row's address resolves in
        // `scene/snapshot`, `scene/query` and `scene/invoke`.
        let Scene::Container(c) = profile_scene() else {
            unreachable!("the fixture is a container")
        };
        for (index, child) in c.children.iter().enumerate() {
            assert_eq!(
                root.children[index].segment.as_deref(),
                Some(child.path_segment_at(index).as_str()),
            );
        }
    }

    #[test]
    fn r1557_a_replayed_subtree_is_attributed_like_an_encoded_one() {
        // The central claim. The measurement is a difference of censuses of the
        // ENCODED scene, so a subtree the fragment cache appended whole lands
        // in it exactly as a freshly-walked one does. A tally kept by the
        // walker would attribute the replay ZERO — the reading a profiler must
        // never produce, because the GPU draws it either way.
        let scene = profile_scene();
        let mut text_cache = LayoutCache::new();
        let mut image_cache = ImageCache::new();
        let mut fragments = FragmentCache::new();

        let mut cold_out = VelloScene::new();
        let mut cold = DrawProfiler::new();
        to_vello_cached_profiled(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut fragments,
            None,
            &mut cold_out,
            true,
            true,
            &mut cold,
        );
        let walked_cold = fragments.nodes_walked_last_paint();

        let mut warm_out = VelloScene::new();
        let mut warm = DrawProfiler::new();
        to_vello_cached_profiled(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut fragments,
            None,
            &mut warm_out,
            true,
            true,
            &mut warm,
        );
        let walked_warm = fragments.nodes_walked_last_paint();

        assert!(
            walked_warm < walked_cold,
            "premise: the second paint must be served by the cache \
             ({walked_cold} -> {walked_warm})",
        );
        let cold_root = cold.finish(Vec::new()).root.expect("root");
        let warm_root = warm.finish(Vec::new()).root.expect("root");
        assert_eq!(
            warm_root.total, cold_root.total,
            "a replayed root draws exactly what it drew when encoded",
        );
        assert!(warm_root.total.glyphs > 0, "…and that is not zero");
        // And the honest consequence, stated rather than hidden: the walk did
        // not ENTER the replayed subtree, so it is attributed whole and not
        // decomposed. That is why `ShellCore::draw_profile_for_window` profiles
        // into a cold cache — a decomposed profile of the same draw work.
        assert!(
            warm_root.children.is_empty(),
            "a cache hit is a node the walk declined to descend through",
        );
        assert_eq!(warm_root.own, warm_root.total);
        assert_eq!(warm_root.own_sum(), warm_root.total);
    }

    #[test]
    fn r1557_a_scroll_owns_its_clip_layer_and_lends_its_address() {
        use pinion_core::scene::ScrollNode;
        let content = Scene::Container(
            ContainerNode::new(vec![Scene::Box(BoxNode::filled(
                Rect::new(0, 0, 50, 50),
                Color::rgb(0, 0xff, 0),
            ))])
            .with_style(BoxStyle::filled(Color::rgb(0x11, 0x22, 0x33))),
        );
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(10, 10, 100, 100), content));
        let (root, encoded) = profile_once(&scene);

        assert_eq!(root.kind, SceneNodeKind::Scroll);
        assert_eq!(root.total, encoded);
        assert_eq!(root.own_sum(), root.total);
        // The viewport clip is pushed by the scroll and belongs to the scroll.
        assert_eq!(
            root.own.layers, 1,
            "the scroll owns the clip layer it pushes"
        );
        assert_eq!(root.children.len(), 1);
        // A `Scroll` consumes no path segment, so its content reports the
        // scroll's own address — the `scene/locate` rule, not an exception
        // invented here.
        assert_eq!(root.children[0].segment, None);
        assert_eq!(root.children[0].own.layers, 0);
        assert!(root.children[0].total.paths > 0, "the content drew boxes");
    }

    #[test]
    fn r1558_a_scoped_profile_equals_the_subtree_of_the_whole_one() {
        // The property `scene/draw_profile`'s `path` scope rests on. Profiling
        // a subtree ALONE has to produce exactly the tree a whole-scene profile
        // holds for it — otherwise scoping is a different measurement wearing
        // the same name, and an agent that drilled down would be comparing two
        // numbers that were never comparable.
        let (whole, _) = profile_once(&nested_profile_scene());
        let in_place = &whole.children[1];
        assert_eq!(
            in_place.tag.as_deref(),
            Some("panel"),
            "premise: child 1 is the panel this test scopes to",
        );
        let (alone, _) = profile_once(&nested_panel());

        // The one field that MUST differ, asserted rather than skipped: in
        // place the panel is its parent's child, alone it is a root and
        // consumes no path segment.
        assert_eq!(in_place.segment.as_deref(), Some("panel"));
        assert_eq!(alone.segment, None);
        assert_same_attribution(&alone, in_place, "panel");

        // …and the premise that makes the equality worth asserting: the panel
        // is a real part of a strictly larger measurement, in every unit.
        assert!(alone.node_count() >= 4, "{}", alone.node_count());
        assert!(whole.node_count() > alone.node_count());
        assert!(alone.total.glyphs > 0 && alone.total.paths > 0);
        assert!(whole.total.glyphs > alone.total.glyphs);
        assert_eq!(alone.own_sum(), alone.total, "and still a partition");
    }

    #[test]
    fn r1558_a_subtrees_cost_does_not_depend_on_its_ancestors() {
        // The reason the equality above holds is a property of the ENCODER,
        // not a convention: an inherited transform moves coordinates and
        // changes no count, and an ancestor's clip is an input to damage
        // reporting rather than a cull. So the same subtree nested under a
        // scrolled, offset, clipping ancestor draws exactly what it draws at
        // the root — which is the case that would break first if either
        // assumption were wrong.
        use pinion_core::scene::ScrollNode;
        let scrolled = Scene::Scroll(
            ScrollNode::new(
                Rect::new(5, 7, 200, 60),
                Scene::Container(ContainerNode::new(vec![nested_panel()])),
            )
            .with_offset(0, 25),
        );
        let (outer, _) = profile_once(&scrolled);
        // Scroll -> content Container (no segment) -> the panel.
        let in_place = &outer.children[0].children[0];
        assert_eq!(in_place.tag.as_deref(), Some("panel"));
        assert!(
            outer.own.layers > 0,
            "premise: the ancestor really did push a clip layer",
        );

        let (alone, _) = profile_once(&nested_panel());
        assert_same_attribution(&alone, in_place, "panel-under-scroll");
        assert_eq!(
            alone.total.layers, in_place.total.layers,
            "the ancestor's clip layer is charged to the ancestor, not folded \
             into the subtree it clips",
        );
    }

    /// One tagless `Scene::Text` leaf, for the draw-census tests.
    fn text_scene(label: &str) -> Scene {
        Scene::Text(TextNode::styled(
            label,
            Rect::new(0, 0, 4000, 40),
            TextStyle::new().with_size_px(16),
        ))
    }

    /// Encode a scene with no fragment cache in play and census the result.
    fn encode_uncached(scene: &Scene) -> DrawWork {
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(scene, &|_| None, &mut cache, &mut vello);
        draw_work_of(&vello)
    }

    // R705 §5.39 — the focus-ring tests moved with the implementation
    // to `pinion_overlay::focus_ring` (the ring is now an introspectable
    // overlay Scene::Box, not an opaque vello stroke emitted here).

    // ----- R51.188 §5.45 R55.E.1 Vello paint clipping tests -----

    #[test]
    fn r55_e1_scroll_arm_walks_content_box_hook() {
        // R55.E.1 — the Scroll arm recurses into `content`. The hook
        // visits every BoxNode inside the scroll's content tree;
        // verifies the walker reaches past the scroll wrapper rather
        // than treating it as a leaf.
        use pinion_core::scene::ScrollNode;
        let inner_box = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 50, 50), Color::rgb(0, 0xff, 0))
                .with_tag("scroll_leaf"),
        );
        let scroll = ScrollNode::new(Rect::new(10, 10, 100, 100), inner_box);
        let scene = Scene::Scroll(scroll);
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        let saw_leaf = Cell::new(false);
        to_vello(
            &scene,
            &|b: &BoxNode| {
                if b.tag.as_deref() == Some("scroll_leaf") {
                    saw_leaf.set(true);
                }
                None
            },
            &mut cache,
            &mut vello,
        );
        assert!(saw_leaf.get(), "scroll content must be visited");
    }

    #[test]
    fn r55_e1_scroll_layer_balances_on_panic_free_walk() {
        // R55.E.1 — Vello's `pop_layer` panics on encoder
        // underflow; if the Scroll arm's `push_clip_layer` and
        // `pop_layer` are not balanced this test trips. Empty
        // content + non-empty content + nested scroll all exit
        // cleanly.
        use pinion_core::scene::ScrollNode;
        let empty_content = Scene::Container(
            ContainerNode::new(vec![]).with_style(BoxStyle::filled(Color::default())),
        );
        let empty_scroll = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 100), empty_content));

        let inner_box = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 200, 200),
            Color::rgb(0xff, 0, 0),
        ));
        let plain_scroll = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 100), inner_box));

        let inner_inner = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 50, 50),
            Color::rgb(0, 0, 0xff),
        ));
        let inner_scroll = Scene::Scroll(ScrollNode::new(Rect::new(10, 10, 50, 50), inner_inner));
        let outer_scroll = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 200, 200), inner_scroll));

        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&empty_scroll, &|_| None, &mut cache, &mut vello);
        to_vello(&plain_scroll, &|_| None, &mut cache, &mut vello);
        to_vello(&outer_scroll, &|_| None, &mut cache, &mut vello);
    }

    #[test]
    fn r55_e1_scroll_text_inside_lays_out_through_cache() {
        // R55.E.1 — Text inside a scroll routes through the same
        // `paint_text` path (now transform-aware). Cache populates
        // once and short-circuits on the repeat walk — same
        // steady-state cache-hit shape as the non-scroll text test.
        use pinion_core::scene::ScrollNode;
        let text = Scene::Text(TextNode::styled(
            "Scrolled text",
            Rect::new(0, 0, 200, 32),
            TextStyle::new().with_size_px(16),
        ));
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), text).with_offset(0, 20);
        let scene = Scene::Scroll(scroll);
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "first paint populates cache");
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "repeat paint hits cache, no growth");
    }

    #[test]
    fn r1068_self_hosted_arm_routes_single_style_text_and_none_is_byte_identical() {
        // R1068 §5.37 — the opt-in self-hosted paint arm, the campaign's
        // production-Scene::Text connection. Forcing consumer (default gate):
        // a single-style `Scene::Text` leaf paints through §5.37 when an engine
        // is supplied (one atlas fill per glyph), and `engine = None` is
        // byte-identical to the pre-R1068 `to_vello` (0 regression). Drives the
        // real production paint entry (`to_vello_with_text_engine`), not a
        // call-and-ignore. Uses the bundled NotoSans fixture so the glyph-count
        // assertion is deterministic (system discovery is environment-dependent;
        // the realgpu seam test carries the system-font pixel proof).
        let font = pinion_text_font::Font::from_bytes(NOTO_FIXTURE.to_vec())
            .expect("parse NotoSans fixture");
        let engine = SelfHostedTextEngine::from_font(font);

        // Default TextStyle = Visible overflow (no clip), so n_paths counts only
        // the glyph fills.
        let scene = Scene::Text(TextNode::styled(
            "Hi",
            Rect::new(0, 0, 200, 32),
            TextStyle::new().with_size_px(16),
        ));

        // 0-regression: `engine = None` is byte-identical to `to_vello` BY
        // CONSTRUCTION — `to_vello` is a one-line forward to
        // `to_vello_with_text_engine(.., None, ..)` (see its body), so the two
        // issue the identical call sequence. This assertion is a sanity check on
        // that forwarding (a path-count is weaker than byte-identity, but a
        // mismatch here would mean the forward broke), not the proof itself.
        let mut parley_default = VelloScene::new();
        let mut cache_a = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache_a, &mut parley_default);

        let mut engine_none = VelloScene::new();
        let mut cache_b = LayoutCache::new();
        to_vello_with_text_engine(&scene, &|_| None, &mut cache_b, None, &mut engine_none);
        assert_eq!(
            engine_none.encoding().n_paths,
            parley_default.encoding().n_paths,
            "engine=None must issue the same path count as to_vello (forwarding intact)"
        );

        // engine = Some routes "Hi" through §5.37: one atlas fill per placed glyph.
        let mut self_hosted = VelloScene::new();
        let mut cache_c = LayoutCache::new();
        to_vello_with_text_engine(
            &scene,
            &|_| None,
            &mut cache_c,
            Some(&engine),
            &mut self_hosted,
        );

        let shaped = pinion_text_font::shape_paragraph_with_fallback(&[engine.font()], "Hi", 16.0);
        let rendered = pinion_text_font::render_paragraph_atlased(&[engine.font()], &shaped, 16.0)
            .expect("atlas-render Hi");
        assert!(!rendered.placed.is_empty(), "Hi atlas-places glyphs");
        assert_eq!(
            self_hosted.encoding().n_paths,
            u32::try_from(rendered.placed.len()).expect("glyph count fits u32"),
            "self-hosted arm emits one fill per placed glyph (routed through §5.37, not parley)"
        );

        // Self-consistent advance: shaped pen-x is monotone left-to-right, so the
        // §5.37 caret/advance never overlaps or reverses (the parity the campaign
        // requires is §5.37's own self-consistency, not byte-parity with parley).
        let xs: Vec<f32> = shaped.glyphs.iter().map(|g| g.x).collect();
        assert!(
            xs.windows(2).all(|w| w[1] >= w[0]),
            "shaped glyph pen-x is monotone (self-consistent advance)"
        );
    }

    #[test]
    fn r1068_self_hosted_arm_falls_through_to_parley_for_styled_runs() {
        // R1068 §5.37 — out-of-scope text (multi-style runs / decorations /
        // multi-line) must take the parley path even with an engine supplied, so
        // the arm never half-renders a case it does not fully handle. A styled-run
        // node with an engine must paint identically to the same node with no
        // engine (both parley).
        let font = pinion_text_font::Font::from_bytes(NOTO_FIXTURE.to_vec())
            .expect("parse NotoSans fixture");
        let engine = SelfHostedTextEngine::from_font(font);

        let runs = vec![pinion_core::scene::StyleRun::new(
            0,
            2,
            TextStyle::new().with_size_px(16),
        )];
        let scene = Scene::Text(
            TextNode::styled(
                "Hi",
                Rect::new(0, 0, 200, 32),
                TextStyle::new().with_size_px(16),
            )
            .with_runs(runs),
        );
        let Scene::Text(node) = &scene else {
            unreachable!()
        };
        assert!(
            !self_hosted_eligible(node, &engine),
            "a styled-run node is out of the self-hosted arm's scope"
        );

        let mut with_engine = VelloScene::new();
        let mut cache_a = LayoutCache::new();
        to_vello_with_text_engine(
            &scene,
            &|_| None,
            &mut cache_a,
            Some(&engine),
            &mut with_engine,
        );

        let mut without_engine = VelloScene::new();
        let mut cache_b = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache_b, &mut without_engine);
        assert_eq!(
            with_engine.encoding().n_paths,
            without_engine.encoding().n_paths,
            "out-of-scope styled-run text falls through to parley unchanged"
        );
    }

    #[test]
    fn r1072_cached_arm_routes_text_through_self_hosted_engine() {
        // R1072 §5.37 — the CACHED production paint walker, now engine-aware. The
        // shell paints through `to_vello_cached*`, so the R1068 uncached arm alone
        // never reached production; this closes that gap. A single-style
        // `Scene::Text` leaf inside the FragmentCache path routes through §5.37
        // when an engine is supplied (one atlas fill per placed glyph), and
        // `engine = None` is byte-identical to `to_vello_cached`. NotoSans fixture
        // keeps the glyph-count deterministic.
        let font = pinion_text_font::Font::from_bytes(NOTO_FIXTURE.to_vec())
            .expect("parse NotoSans fixture");
        let engine = SelfHostedTextEngine::from_font(font);

        let scene = Scene::Text(TextNode::styled(
            "Hi",
            Rect::new(0, 0, 200, 32),
            TextStyle::new().with_size_px(16),
        ));

        // engine = None : byte-identical to the engine-free cached walker (the
        // delegation `to_vello_cached` → `..with_text_engine(None)` stays intact).
        let mut none_cached = VelloScene::new();
        to_vello_cached_with_text_engine(
            &scene,
            &|_| None,
            &mut LayoutCache::new(),
            &mut ImageCache::new(),
            &mut FragmentCache::new(),
            None,
            &mut none_cached,
            true,
            true,
        );
        let mut plain_cached = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut LayoutCache::new(),
            &mut ImageCache::new(),
            &mut FragmentCache::new(),
            &mut plain_cached,
        );
        assert_eq!(
            none_cached.encoding().n_paths,
            plain_cached.encoding().n_paths,
            "engine=None cached walk == to_vello_cached (forwarding intact)"
        );

        // engine = Some : routes "Hi" through §5.37 — one atlas fill per glyph.
        let mut self_hosted = VelloScene::new();
        to_vello_cached_with_text_engine(
            &scene,
            &|_| None,
            &mut LayoutCache::new(),
            &mut ImageCache::new(),
            &mut FragmentCache::new(),
            Some(&engine),
            &mut self_hosted,
            true,
            true,
        );
        let shaped = pinion_text_font::shape_paragraph_with_fallback(&[engine.font()], "Hi", 16.0);
        let rendered = pinion_text_font::render_paragraph_atlased(&[engine.font()], &shaped, 16.0)
            .expect("atlas-render Hi");
        assert!(!rendered.placed.is_empty(), "Hi atlas-places glyphs");
        assert_eq!(
            self_hosted.encoding().n_paths,
            u32::try_from(rendered.placed.len()).expect("glyph count fits u32"),
            "cached self-hosted arm emits one fill per placed glyph (routed through §5.37)"
        );
    }

    #[test]
    fn r1072_cached_engine_fragment_replays_on_cache_hit() {
        // R1072.1 — the cache-HIT (replay) path, not just the miss the sibling
        // test covers. The other cached test paints a BARE `Scene::Text` (never a
        // Container, so the FragmentCache fast-path is never entered). Here a
        // cacheable Container wraps the §5.37 leaf and is painted twice through ONE
        // cache: frame 1 = miss (encode the §5.37 glyph fills into the stored
        // fragment), frame 2 = hit (append the stored fragment). This proves the
        // "engine fragment stays valid for replay" claim — the replayed scene is
        // bit-for-bit the §5.37 encode, not a parley re-render.
        let font = pinion_text_font::Font::from_bytes(NOTO_FIXTURE.to_vec())
            .expect("parse NotoSans fixture");
        let engine = SelfHostedTextEngine::from_font(font);

        let scene = Scene::Container(ContainerNode::new(vec![Scene::Text(TextNode::styled(
            "Hi",
            Rect::new(0, 0, 200, 32),
            TextStyle::new().with_size_px(16),
        ))]));
        assert!(
            scene.is_cacheable_for_paint(),
            "a Container of static text is cacheable"
        );

        let mut text_cache = LayoutCache::new();
        let mut image_cache = ImageCache::new();
        let mut fragment_cache = FragmentCache::new();

        // Frame 1: cache miss — encodes the subtree incl. the §5.37 per-glyph fills.
        let mut frame1 = VelloScene::new();
        to_vello_cached_with_text_engine(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut fragment_cache,
            Some(&engine),
            &mut frame1,
            true,
            true,
        );
        assert_eq!(fragment_cache.misses(), 1, "first paint is a miss");
        assert_eq!(fragment_cache.hits(), 0);

        // Frame 2: cache hit — replays the stored fragment (no re-encode).
        let mut frame2 = VelloScene::new();
        to_vello_cached_with_text_engine(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut fragment_cache,
            Some(&engine),
            &mut frame2,
            true,
            true,
        );
        assert_eq!(
            fragment_cache.hits(),
            1,
            "second identical paint is a hit (fragment replayed)"
        );

        // The replayed frame carries the SAME path count as the miss encode — the
        // §5.37 fragment survived caching (a parley re-render would differ).
        assert_eq!(
            frame1.encoding().n_paths,
            frame2.encoding().n_paths,
            "the hit replay reproduces the §5.37 miss encode"
        );
        let shaped = pinion_text_font::shape_paragraph_with_fallback(&[engine.font()], "Hi", 16.0);
        let rendered = pinion_text_font::render_paragraph_atlased(&[engine.font()], &shaped, 16.0)
            .expect("atlas-render Hi");
        assert!(
            frame2.encoding().n_paths >= u32::try_from(rendered.placed.len()).expect("fits u32"),
            "the replayed fragment carries the §5.37 per-glyph fills"
        );
    }

    #[test]
    fn r1072_caret_bearing_text_declines_self_hosted_arm() {
        // R1072 §5.37 — the R1070.1 caret contract at the PAINT arm: a
        // caret-bearing (editable) leaf is otherwise eligible (single-style /
        // single-line / Start / Normal / undecorated) but must NOT route through
        // §5.37 — its caret / selection geometry is shaped by parley, so the
        // glyphs must stay parley too or the caret drifts off them. With an engine
        // supplied it paints byte-identically to the same node with no engine.
        let font = pinion_text_font::Font::from_bytes(NOTO_FIXTURE.to_vec())
            .expect("parse NotoSans fixture");
        let engine = SelfHostedTextEngine::from_font(font);

        let scene = Scene::Text(
            TextNode::styled(
                "Hi",
                Rect::new(0, 0, 200, 32),
                TextStyle::new().with_size_px(16),
            )
            .caret_bearing(),
        );
        let Scene::Text(node) = &scene else {
            unreachable!()
        };
        assert!(
            !self_hosted_eligible(node, &engine),
            "a caret-bearing node is excluded from the self-hosted arm"
        );

        let mut with_engine = VelloScene::new();
        to_vello_with_text_engine(
            &scene,
            &|_| None,
            &mut LayoutCache::new(),
            Some(&engine),
            &mut with_engine,
        );
        let mut without_engine = VelloScene::new();
        to_vello(
            &scene,
            &|_| None,
            &mut LayoutCache::new(),
            &mut without_engine,
        );
        assert_eq!(
            with_engine.encoding().n_paths,
            without_engine.encoding().n_paths,
            "caret-bearing text falls through to parley unchanged (caret stays aligned)"
        );
    }

    #[test]
    fn r639_fill_rect_zero_radius_path_is_sharp() {
        // R639 §5.16 §5.2 — corner_radius == 0 must paint a sharp
        // [`KurboRect`] (legacy zero-cost path). Walk a single-box
        // scene through `to_vello` with the default `BoxStyle`
        // (corner_radius = 0) and confirm the vello scene is non-
        // empty (the fill emit succeeded) and the scene's recorded
        // primitive count matches the rounded variant — both paths
        // emit one fill, only the underlying shape differs.
        let scene = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 100, 40),
            Color::rgb(0xff, 0, 0),
        ));
        assert_eq!(
            scene.box_style().map(|s| s.corner_radius),
            Some(0),
            "default BoxStyle radius is 0",
        );
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        // The sharp-rect path was the only one before R639; survival
        // through the new dispatch + non-empty vello scene proves
        // the zero-radius arm preserved its legacy behaviour.
        assert!(
            vello.encoding().n_paths > 0,
            "fill emitted at least one path"
        );
    }

    #[test]
    fn r639_fill_rect_nonzero_radius_emits_rounded_rect() {
        // R639 §5.16 §5.2 — corner_radius > 0 must paint a
        // [`KurboRoundedRect`]. Vello records the path shape in its
        // encoding; the rounded-rect path produces a different
        // primitive count from the sharp-rect path (kurbo decomposes
        // rounded corners into cubic Béziers).
        let sharp = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 109, 40),
            Color::rgb(103, 80, 164),
        ));
        let rounded = Scene::Box(BoxNode::new(
            Rect::new(0, 0, 109, 40),
            BoxStyle::filled(Color::rgb(103, 80, 164)).with_corner_radius(100),
        ));
        let mut cache = LayoutCache::new();
        let mut sharp_scene = VelloScene::new();
        to_vello(&sharp, &|_| None, &mut cache, &mut sharp_scene);
        let mut rounded_scene = VelloScene::new();
        to_vello(&rounded, &|_| None, &mut cache, &mut rounded_scene);
        // The sharp rect = 4 line segments (1 path with 4 verbs).
        // The rounded rect = 4 line segments + 4 quadrant arcs (1
        // path with more verbs). The exact verb counts vary across
        // kurbo versions; assert only that the rounded encoding is
        // strictly larger than the sharp one — wire-up evidence
        // independent of internal vello/kurbo version details.
        assert!(
            rounded_scene.encoding().n_path_segments > sharp_scene.encoding().n_path_segments,
            "rounded path must have more segments than sharp; \
             sharp={}, rounded={}",
            sharp_scene.encoding().n_path_segments,
            rounded_scene.encoding().n_path_segments,
        );
    }

    // R1516 — the R639 assertion above used to reach its `corner_radius`
    // through a local `MatchBoxStyle` trait whose comment said it lived in
    // the test module "to keep the production surface free of one-off
    // accessors". It was not one-off: the same `Box | Container | _ => 0`
    // match existed in `pinion-overlay`'s focus-ring walk, and a third copy
    // was about to be written for the backend-parity matrix.
    // `Scene::box_style` is that accessor, published where the variant list
    // is knowable, so the local trait is gone.

    #[test]
    fn r55_e1_scroll_arm_survives_offset_overshoot() {
        // R55.E.1 — adversarial offset (content shifted past the
        // viewport entirely) still completes the walk without
        // panic. The walker does not clamp the offset — that's
        // ScrollState's job — so paint just renders the empty
        // visible region.
        use pinion_core::scene::ScrollNode;
        let content = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 50, 50),
            Color::rgb(0, 0xff, 0),
        ));
        let scroll =
            ScrollNode::new(Rect::new(0, 0, 100, 100), content).with_offset(i32::MAX, i32::MAX);
        let scene = Scene::Scroll(scroll);
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
    }

    // ─────────────────────────────────────────────────────────────
    // R682 §5.16 atomic 1 — FragmentCache + to_vello_cached
    // ─────────────────────────────────────────────────────────────

    use pinion_core::scene::{EffectNode, ExternalNode, ImmediateMode, ImmediateModeNode, Scene};

    fn null_hook<'a>() -> &'a (dyn Fn(&BoxNode) -> Option<Color> + 'a) {
        &|_b: &BoxNode| None
    }

    fn simple_container() -> Scene {
        Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(
                Rect::new(10, 10, 100, 50),
                Color::rgb(0xff, 0, 0),
            )),
            Scene::Text(TextNode::new("hi", Rect::new(10, 70, 100, 20))),
        ]))
    }

    fn mutated_container() -> Scene {
        Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(
                Rect::new(10, 10, 100, 50),
                // Color differs from `simple_container`.
                Color::rgb(0, 0xff, 0),
            )),
            Scene::Text(TextNode::new("hi", Rect::new(10, 70, 100, 20))),
        ]))
    }

    // ── R1538 §5.16 — the encode walk's node census ──

    /// Count `Scene` nodes independently of the encoder, so the census is
    /// checked against a second observation rather than against itself.
    fn scene_nodes_of(scene: &Scene) -> u32 {
        match scene {
            Scene::Container(c) => c.children.iter().map(scene_nodes_of).sum::<u32>() + 1,
            Scene::Scroll(s) => scene_nodes_of(s.content.as_ref()) + 1,
            _ => 1,
        }
    }

    fn paint_once(scene: &Scene, cache: &mut FragmentCache, text: &mut LayoutCache) {
        let mut vello = VelloScene::new();
        to_vello_cached(
            scene,
            &null_hook(),
            text,
            &mut ImageCache::new(),
            cache,
            &mut vello,
        );
    }

    #[test]
    fn r1538_a_cold_paint_walks_the_whole_tree() {
        // Nothing cached: the walk must reach every node, and the census must
        // equal an independent count of the same scene. Asserting it against a
        // number this walk produced would pass however the walk is wrong.
        let scene = simple_container();
        let mut cache = FragmentCache::new();
        paint_once(&scene, &mut cache, &mut LayoutCache::new());
        assert_eq!(scene_nodes_of(&scene), 3, "fixture shape pinned");
        assert_eq!(cache.nodes_walked_last_paint(), 3);
    }

    #[test]
    fn r1538_a_served_paint_walks_only_what_the_cache_did_not_serve() {
        // The whole point. A hit at the root short-circuits its subtree, so
        // the second paint of an identical scene walks ONE node against three
        // — and that ratio is what says the fragment cache worked on THIS
        // frame. The hit rate cannot say it: 100% is 100% whether the fragment
        // replayed was a label or the entire window.
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        paint_once(&simple_container(), &mut cache, &mut text);
        let cold = cache.nodes_walked_last_paint();
        paint_once(&simple_container(), &mut cache, &mut text);
        let warm = cache.nodes_walked_last_paint();

        assert_eq!(cold, 3);
        assert_eq!(warm, 1, "the hit counts, the subtree it declines does not");
        assert!(warm < cold);
        assert_eq!(cache.hits(), 1);
        assert!(
            (cache.hit_rate() - 0.5).abs() < f32::EPSILON,
            "and the hit rate is unchanged by any of this — a different axis",
        );
    }

    #[test]
    fn r1538_census_is_per_paint_and_does_not_accumulate() {
        // A cumulative counter and a per-paint one read the same on frame one
        // and diverge forever after. Three identical paints must each report
        // their own walk, not the sum.
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        paint_once(&simple_container(), &mut cache, &mut text);
        paint_once(&simple_container(), &mut cache, &mut text);
        paint_once(&simple_container(), &mut cache, &mut text);
        assert_eq!(cache.nodes_walked_last_paint(), 1);
        assert_eq!(cache.paint_count(), 3, "three paints did happen");
    }

    #[test]
    fn r1538_a_changed_scene_re_walks_what_it_invalidated() {
        // A cache MISS on the second paint must restore the full walk — the
        // census tracks the walk, not the frame number.
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        paint_once(&simple_container(), &mut cache, &mut text);
        paint_once(&mutated_container(), &mut cache, &mut text);
        assert_eq!(cache.nodes_walked_last_paint(), 3);
        assert_eq!(cache.misses(), 2);
    }

    #[test]
    fn r682_fragment_cache_starts_empty() {
        let cache = FragmentCache::new();
        assert_eq!(cache.entries(), 0);
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 0);
        assert_eq!(cache.paint_count(), 0);
        assert!((cache.hit_rate() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn r682_first_paint_is_a_cache_miss() {
        let scene = simple_container();
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut vello,
        );
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.entries(), 1, "miss installs the fragment");
        assert_eq!(cache.paint_count(), 1);
    }

    #[test]
    fn r682_second_identical_paint_is_a_cache_hit() {
        // Two paints of structurally identical scenes hit the cache
        // on the second call — the killer property of the §5.16
        // dirty-subtree-cache substrate. The shell-side V::view
        // re-runs every frame (R26 contract) but `paint_hash`
        // matches, so the encoded fragment replays without fresh
        // walk.
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();

        let mut vello1 = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut vello1,
        );
        let mut vello2 = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut vello2,
        );

        assert_eq!(cache.hits(), 1, "second paint hits");
        assert_eq!(cache.misses(), 1, "first paint missed");
        assert_eq!(cache.paint_count(), 2);
        // Hit rate after 1 hit + 1 miss = 50%.
        assert!((cache.hit_rate() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn r682_mutated_scene_is_a_cache_miss() {
        // A field change anywhere in the cacheable subtree changes
        // the structural hash, so the second paint must miss.
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();

        let mut vello1 = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut vello1,
        );
        let mut vello2 = VelloScene::new();
        to_vello_cached(
            &mutated_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut vello2,
        );

        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 2);
        // Sweep at end of paint 2 evicts the unreached paint-1
        // fragment, so only the paint-2 fragment remains.
        assert_eq!(cache.entries(), 1);
    }

    #[test]
    fn r682_mark_and_sweep_evicts_unreached_entries() {
        // Paint scene A (installs fragment A), then paint scene B
        // (installs B and evicts A because A's hash was not consulted
        // during paint 2). Verify the cache holds only B's fragment
        // after paint 2.
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();

        let mut v = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert_eq!(cache.entries(), 1);

        let mut v = VelloScene::new();
        to_vello_cached(
            &mutated_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        // Only mutated_container's fragment survives — simple_container's
        // entry was not consulted this paint, sweep dropped it.
        assert_eq!(cache.entries(), 1);
    }

    /// Immediate-mode driver stub for cacheability tests.
    #[derive(Debug, Default)]
    struct InertImmediate;

    impl ImmediateMode for InertImmediate {}

    #[test]
    fn r682_immediate_mode_subtree_is_not_cached() {
        // §2 #4 substrate contract: a Container containing an
        // ImmediateModeNode is not cacheable — its paint changes per
        // tick. The cache must not install a fragment for the parent.
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(
                Rect::new(0, 0, 50, 50),
                Color::rgb(0, 0, 0xff),
            )),
            Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
                InertImmediate,
                Rect::new(0, 50, 50, 50),
            )),
        ]));
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        let mut v = VelloScene::new();
        to_vello_cached(
            &scene,
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        // No miss → no install. Cache stays empty.
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 0);
        assert_eq!(cache.entries(), 0);
    }

    #[test]
    fn r682_external_subtree_is_not_cached() {
        // §3 opaque-escape: a Container with an External child is
        // not cacheable — the embedded handle's paint is opaque to
        // the cache.
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(Rect::new(0, 0, 50, 50), Color::default())),
            Scene::External(ExternalNode::new(Box::new(
                pinion_core::external::StubExternal::new(),
            ))),
        ]));
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        let mut v = VelloScene::new();
        to_vello_cached(
            &scene,
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert_eq!(cache.entries(), 0);
    }

    #[test]
    fn r682_nested_cacheable_subtree_caches_under_uncacheable_parent() {
        // §2 #4 killer use case: parent Container holds an
        // ImmediateModeNode (uncacheable) AND a child Container that
        // is itself cacheable (only Box / Text inside). The child's
        // subtree MUST install in the cache so the next paint hits.
        // This is the immediate-mode coexistence promise: the live
        // animation paints fresh while the static retained subtree
        // skips re-encoding.
        let make_scene = || {
            Scene::Container(ContainerNode::new(vec![
                Scene::Container(
                    ContainerNode::new(vec![Scene::Text(TextNode::new(
                        "header",
                        Rect::new(0, 0, 200, 24),
                    ))])
                    .with_tag("retained_header"),
                ),
                Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
                    InertImmediate,
                    Rect::new(0, 30, 200, 100),
                )),
            ]))
        };

        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();

        let mut v = VelloScene::new();
        to_vello_cached(
            &make_scene(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        // First paint: header Container missed (installed) — root
        // Container is uncacheable so it didn't probe; ImmediateMode
        // doesn't probe.
        assert_eq!(cache.misses(), 1, "header Container installed");
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.entries(), 1);

        let mut v = VelloScene::new();
        to_vello_cached(
            &make_scene(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        // Second paint: header hits.
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.entries(), 1);
    }

    #[test]
    fn r682_cache_clear_resets_counters_and_entries() {
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        let mut v = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert!(cache.entries() > 0);

        cache.clear();
        assert_eq!(cache.entries(), 0);
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 0);
        assert_eq!(cache.paint_count(), 0);
    }

    #[test]
    fn r682_uncached_path_still_paints_same_output() {
        // Encoding identical scenes through to_vello (no cache) and
        // to_vello_cached (with cache) produces the same `vello::Scene`
        // payload — the cache is a perf substrate, not a behaviour
        // change. We can't compare VelloScene contents directly
        // (no PartialEq), but we can sanity-check that neither path
        // panics and both reach the same paint count.
        let scene = simple_container();
        let mut text = LayoutCache::new();

        let mut uncached = VelloScene::new();
        to_vello(&scene, &null_hook(), &mut text, &mut uncached);

        let mut cache = FragmentCache::new();
        let mut cached = VelloScene::new();
        to_vello_cached(
            &scene,
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut cached,
        );

        // Both produced a valid VelloScene; the cached version
        // installed exactly one fragment (the root Container).
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.entries(), 1);
    }

    /// (R1520 §5.16) A scrolled subtree caches, and keeps hitting when the
    /// offset moves — the R682+1 carry, closed.
    ///
    /// This is the discriminating test for the change: R682's cache boundary
    /// required `Affine::IDENTITY`, and a `Scene::Scroll` hands its content a
    /// translation, so before R1520 the second paint below reported `hits ==
    /// 0` and `entries == 0` — a scrolled list re-encoded every node every
    /// frame. Both assertions fail on a revert.
    ///
    /// The offsets differ between the two paints on purpose. Equal offsets
    /// would also hit with the transform *baked into* the key, so they cannot
    /// tell "the cache learned about scrolling" from "the cache is keyed on
    /// the offset too". A hit at a *different* offset is only possible if the
    /// stored fragment is transform-free.
    #[test]
    fn r1520_scrolled_content_hits_the_cache_after_the_offset_moves() {
        let content = || {
            Scene::Container(ContainerNode::new(vec![
                Scene::Box(BoxNode::filled(
                    Rect::new(0, 0, 80, 20),
                    Color::rgb(9, 9, 9),
                )),
                Scene::Box(BoxNode::filled(
                    Rect::new(0, 30, 80, 20),
                    Color::rgb(7, 7, 7),
                )),
            ]))
        };
        let at_offset = |dy: i32| {
            use pinion_core::scene::ScrollNode;
            Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 40), content()).with_offset(0, dy))
        };

        let mut text = LayoutCache::new();
        let mut images = ImageCache::new();
        let mut cache = FragmentCache::new();

        let mut first = VelloScene::new();
        to_vello_cached(
            &at_offset(10),
            &null_hook(),
            &mut text,
            &mut images,
            &mut cache,
            &mut first,
        );
        assert_eq!(
            (cache.hits(), cache.misses(), cache.entries()),
            (0, 1, 1),
            "the first scrolled paint encodes the content container once and stores it"
        );

        let mut second = VelloScene::new();
        to_vello_cached(
            &at_offset(25),
            &null_hook(),
            &mut text,
            &mut images,
            &mut cache,
            &mut second,
        );
        assert_eq!(
            (cache.hits(), cache.misses(), cache.entries()),
            (1, 1, 1),
            "scrolling to a new offset re-places the SAME fragment: one hit, no new encode"
        );
    }

    /// (R1520 §5.16) The same content painted bare and inside a scroll shares
    /// one cache entry.
    ///
    /// The positive statement of "fragments are transform-free": if a stored
    /// fragment carried the transform it was placed under, these two would
    /// have to be separate entries. One entry + one hit is the evidence that
    /// the transform lives at the placement, not in the fragment. It also
    /// makes the dedup itself explicit, so a future round that adds the
    /// transform to the cache key has to delete this test on purpose.
    #[test]
    fn r1520_bare_and_scrolled_placements_share_one_fragment() {
        use pinion_core::scene::ScrollNode;

        let content = || {
            Scene::Container(ContainerNode::new(vec![Scene::Box(BoxNode::filled(
                Rect::new(0, 0, 60, 24),
                Color::rgb(3, 4, 5),
            ))]))
        };
        let mut text = LayoutCache::new();
        let mut images = ImageCache::new();
        let mut cache = FragmentCache::new();

        let mut bare = VelloScene::new();
        to_vello_cached(
            &content(),
            &null_hook(),
            &mut text,
            &mut images,
            &mut cache,
            &mut bare,
        );
        assert_eq!((cache.misses(), cache.entries()), (1, 1));

        let scrolled =
            Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 60, 24), content()).with_offset(0, 12));
        let mut placed = VelloScene::new();
        to_vello_cached(
            &scrolled,
            &null_hook(),
            &mut text,
            &mut images,
            &mut cache,
            &mut placed,
        );
        assert_eq!(
            (cache.hits(), cache.misses(), cache.entries()),
            (1, 1, 1),
            "the scrolled placement re-uses the bare paint's fragment"
        );
    }

    /// (R1520 §5.16 R682 atomic 2) A scrolled miss reports damage where its
    /// pixels land, not where its own rect says.
    ///
    /// `last_damage_region` exists for a consumer that uploads pixels, so it
    /// is in screen space, and this pins BOTH steps of getting there. The
    /// content container sits at its own `(0, 0)` and is placed at `y = 30`
    /// (viewport 40 minus offset 10), so it spans screen `30..50` — and the
    /// viewport starts at 40, so its top 10px are clipped away and cannot
    /// differ. `40..50` is the answer; `30..50` (placed, unclipped) and
    /// `0..20` (unplaced) are the two ways to get it wrong, and both look
    /// perfectly plausible written down.
    #[test]
    fn r1520_scrolled_damage_region_is_in_screen_space() {
        use pinion_core::scene::ScrollNode;

        // The container carries its own rect: `insert_miss` reports the
        // *container's* bounds (R682 atomic 2), so a default zero rect would
        // make the region zero-sized regardless of the transform.
        let mut container = ContainerNode::new(vec![Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 50, 20),
            Color::rgb(1, 2, 3),
        ))]);
        container.rect = Rect::new(0, 0, 50, 20);
        let content = Scene::Container(container);
        let scene =
            Scene::Scroll(ScrollNode::new(Rect::new(0, 40, 50, 20), content).with_offset(0, 10));
        let mut cache = FragmentCache::new();
        let mut out = VelloScene::new();
        to_vello_cached(
            &scene,
            &null_hook(),
            &mut LayoutCache::new(),
            &mut ImageCache::new(),
            &mut cache,
            &mut out,
        );
        // viewport.y (40) - offset_y (10) = +30 on the content's own y = 0.
        assert_eq!(
            cache.stats().last_damage_region,
            Some(Rect::new(0, 40, 50, 10)),
            "the damage rect is the container's rect placed by the scroll AND clipped to the viewport"
        );
    }

    /// (R1520 §5.16 R682 atomic 2) A container nested inside a scrolled
    /// fragment reports damage on the SCREEN, not in its fragment.
    ///
    /// The trap R1520's own mechanism sets: inside a cached fragment the
    /// children recurse with the encoding transform reset to `IDENTITY`, so a
    /// nested miss reported against that transform lands at the content's own
    /// coordinates — off by the entire scroll offset, and plausible-looking.
    /// The walk carries a second, never-reset `placement` transform for this.
    ///
    /// The shape is the real virtual-list one: the scroll content is the
    /// outer cached fragment and a row is a nested cacheable container
    /// inside it.
    #[test]
    fn r1520_nested_damage_inside_a_scrolled_fragment_is_placed() {
        use pinion_core::scene::ScrollNode;

        let mut row = ContainerNode::new(vec![Scene::Box(BoxNode::filled(
            Rect::new(0, 100, 40, 10),
            Color::rgb(2, 2, 2),
        ))]);
        row.rect = Rect::new(0, 100, 40, 10);
        let mut outer = ContainerNode::new(vec![Scene::Container(row)]);
        // Deliberately SHORTER than the row's position: the union's far edge
        // then comes from the nested row alone, so the assertion below reads
        // the row's placement and not the outer container's.
        outer.rect = Rect::new(0, 60, 40, 10);
        let scene = Scene::Scroll(
            ScrollNode::new(Rect::new(0, 0, 40, 50), Scene::Container(outer)).with_offset(0, 60),
        );

        let mut cache = FragmentCache::new();
        let mut out = VelloScene::new();
        to_vello_cached(
            &scene,
            &null_hook(),
            &mut LayoutCache::new(),
            &mut ImageCache::new(),
            &mut cache,
            &mut out,
        );

        // Both containers missed. The union runs from the outer container
        // placed at 60 - 60 = 0 to the nested row's placed bottom,
        // 100 + 10 - 60 = 50. Reported against the fragment's own IDENTITY
        // the row would contribute 100..110 and stretch the union to 110 —
        // the value this test exists to exclude.
        assert_eq!(cache.misses(), 2, "outer container + nested row both miss");
        let dmg = cache
            .stats()
            .last_damage_region
            .expect("a miss publishes damage");
        assert_eq!(
            dmg.y.saturating_add(dmg.h),
            50,
            "the nested row's damage is placed by the scroll translation, got {dmg:?}"
        );
    }

    /// (R1520 §5.16 R682 atomic 2) A scrolled container's damage is bounded by
    /// the viewport it is scrolled inside, not by its content extent.
    ///
    /// This is the number letting the cache into scrolled subtrees would
    /// otherwise have wrecked. The content container below is 400px of rows
    /// inside a 50px viewport — the virtual-list shape, where the real ratio is
    /// 320,000px of rows inside a 460px window. Its own rect is the honest
    /// answer to "how tall is this container" and the wrong answer to "which
    /// pixels changed", because everything outside the viewport is clipped away
    /// and cannot differ.
    #[test]
    fn r1520_scrolled_damage_is_bounded_by_the_viewport() {
        use pinion_core::scene::ScrollNode;

        let mut content = ContainerNode::new(vec![Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 40, 400),
            Color::rgb(4, 4, 4),
        ))]);
        content.rect = Rect::new(0, 0, 40, 400);
        let scene = Scene::Scroll(
            ScrollNode::new(Rect::new(0, 10, 40, 50), Scene::Container(content)).with_offset(0, 0),
        );

        let mut cache = FragmentCache::new();
        let mut out = VelloScene::new();
        to_vello_cached(
            &scene,
            &null_hook(),
            &mut LayoutCache::new(),
            &mut ImageCache::new(),
            &mut cache,
            &mut out,
        );

        assert_eq!(
            cache.stats().last_damage_region,
            Some(Rect::new(0, 10, 40, 50)),
            "the damage region is the viewport, not the 400px content extent"
        );
    }

    /// (R1520 §5.16) `screen_bounds` maps the whole rect, not the origin.
    #[test]
    fn r1520_screen_bounds_covers_the_transformed_rect() {
        let r = Rect::new(10, 20, 30, 40);

        assert_eq!(
            screen_bounds(r, Affine::IDENTITY),
            r,
            "identity is the rect itself"
        );
        assert_eq!(
            screen_bounds(r, Affine::translate((5.0, -6.0))),
            Rect::new(15, 14, 30, 40),
            "a pure translation is exact"
        );
        // Scrolled far enough that the top-left leaves the framebuffer. `Rect`
        // is unsigned (§5.2), so the origin clamps to 0 — and the FAR edge has
        // to be preserved, or the reported region under-covers the part still
        // on screen. `w` shrinks to `x1 - 0` rather than staying 30.
        assert_eq!(
            screen_bounds(r, Affine::translate((-25.0, -100.0))),
            Rect::new(0, 0, 15, 0),
            "off-screen halves clamp at 0 while the on-screen far edge survives"
        );
        // A rotation maps the rect to a parallelogram; the answer is the
        // bounding box of the four mapped corners. A quarter turn about the
        // origin sends (10..40, 20..60) to (-60..-20, 10..40), whose visible
        // part is empty in x — the point being that all four corners are
        // consulted, not just the origin.
        let quarter = Affine::rotate(std::f64::consts::FRAC_PI_2);
        let rotated = screen_bounds(r, quarter);
        assert_eq!(
            (rotated.y, rotated.h),
            (10, 30),
            "the mapped y-extent comes from the corners, got {rotated:?}"
        );
    }

    // ---------------------------------------------------------------
    // R1527 §5.16 — the mark phase has a trace step.
    //
    // A row grid: one cacheable Container per row under one cacheable
    // root, which is the shape every Model/View binding paints and the
    // shape that made R682's "consulted" proxy diverge from its own
    // stated "painted" bound. `sel` marks one row's content as changed
    // so a frame can move the selection by one row.
    // ---------------------------------------------------------------

    fn grid_row(i: u32, sel: Option<u32>) -> Scene {
        let fill = if Some(i) == sel {
            Color::rgb(0, 0, 255)
        } else {
            Color::rgb(255, 255, 255)
        };
        Scene::Container(
            ContainerNode::new(vec![Scene::Box(BoxNode::filled(
                Rect::new(0, i * 10, 400, 10),
                fill,
            ))])
            .with_tag(format!("row_{i}")),
        )
    }

    fn row_grid(rows: u32, sel: Option<u32>) -> Scene {
        let children = (0..rows).map(|i| grid_row(i, sel)).collect();
        Scene::Container(ContainerNode::new(children).with_tag("grid_root"))
    }

    /// Paint `scene` once through the production cached walk, returning
    /// `(hits, misses, entries)` for that frame alone.
    fn paint_frame(
        scene: &Scene,
        frag: &mut FragmentCache,
        text: &mut LayoutCache,
    ) -> (u64, u64, usize) {
        let (h0, m0) = (frag.hits(), frag.misses());
        let mut v = VelloScene::new();
        to_vello_cached(
            scene,
            &null_hook(),
            text,
            &mut ImageCache::new(),
            frag,
            &mut v,
        );
        (frag.hits() - h0, frag.misses() - m0, frag.stats().entries)
    }

    /// The collapse itself: one idle frame used to evict every fragment
    /// the root subsumed, because the hit that served them stopped the
    /// walk before it could consult them.
    #[test]
    fn r1527_an_idle_frame_keeps_the_fragments_its_root_subsumes() {
        let scene = row_grid(40, None);
        let (mut frag, mut text) = (FragmentCache::new(), LayoutCache::new());

        let (_, misses, entries) = paint_frame(&scene, &mut frag, &mut text);
        assert_eq!(misses, 41, "first paint installs the root + 40 rows");
        assert_eq!(entries, 41);

        let (hits, misses, entries) = paint_frame(&scene, &mut frag, &mut text);
        assert_eq!((hits, misses), (1, 0), "the root answers the whole frame");
        assert_eq!(
            entries, 41,
            "the 40 rows the root replayed were painted, so they survive the \
             sweep; pre-R1527 this collapsed to 1"
        );

        // Idempotent — a second idle frame is not a slower leak.
        let (_, _, entries) = paint_frame(&scene, &mut frag, &mut text);
        assert_eq!(entries, 41);
    }

    /// What the collapse cost, and the load-bearing assertion of the
    /// round: after idling, changing ONE row reused nothing. Both
    /// numbers discriminate — pre-R1527 this frame was `(0, 41)`.
    #[test]
    fn r1527_one_changed_row_reuses_every_other_rows_fragment() {
        let (mut frag, mut text) = (FragmentCache::new(), LayoutCache::new());
        paint_frame(&row_grid(40, None), &mut frag, &mut text);
        paint_frame(&row_grid(40, None), &mut frag, &mut text);

        // A selection lands on row 7: the root's hash changes, so the
        // walk descends — and finds 39 rows it painted last frame.
        let (hits, misses, entries) = paint_frame(&row_grid(40, Some(7)), &mut frag, &mut text);
        assert_eq!(
            (hits, misses),
            (39, 2),
            "39 unchanged rows hit; the root and row 7 are the only encodes"
        );
        assert_eq!(entries, 41, "the grid is still fully cached");
    }

    /// The other half of the invariant, and the one a trace can break:
    /// reachability must not become retention. Content that leaves the
    /// tree is still collected, so the bound is still "what this frame
    /// painted" and not "what any frame ever painted".
    #[test]
    fn r1527_fragments_of_a_dropped_subtree_are_collected_with_it() {
        let (mut frag, mut text) = (FragmentCache::new(), LayoutCache::new());
        paint_frame(&row_grid(40, None), &mut frag, &mut text);
        let (_, _, entries) = paint_frame(&row_grid(40, None), &mut frag, &mut text);
        assert_eq!(entries, 41);

        // The grid shrinks to 4 rows. The 36 dropped row fragments are
        // reachable from nothing live — the old root that subsumed them
        // is itself gone — so both they and its edges go.
        let (_, _, entries) = paint_frame(&row_grid(4, None), &mut frag, &mut text);
        assert_eq!(
            entries, 5,
            "a fragment survives on being painted, not on having been painted"
        );

        // And the edge map does not outlive the fragments it describes.
        // Stated as the invariant rather than a count, so it holds
        // whatever the tree's shape: every edge set belongs to a live
        // fragment, and every hash it points at is a live fragment too.
        for (owner, children) in &frag.subsumes {
            assert!(
                frag.fragments.contains_key(owner),
                "an edge set outlived the fragment it describes"
            );
            for child in children {
                assert!(
                    frag.fragments.contains_key(child),
                    "an edge points at a fragment that was swept"
                );
            }
        }
        // R1527.1 — and a leaf carries no edge set at all. Only the root
        // subsumes anything in this scene, so one entry, not five.
        assert_eq!(
            frag.subsumes.len(),
            1,
            "leaf fragments store no edge set (pre-R1527.1: one each)"
        );
    }

    // ---------------------------------------------------------------
    // R1531 §5.36 — the paint replays a draw list it does not rebuild.
    // ---------------------------------------------------------------

    /// A text row grid: the shape a Model/View binding paints, and the shape
    /// whose re-encode cost R1531 is about. `sel` marks one row changed so a
    /// frame can move a selection without changing a single string.
    fn text_row_grid(rows: u32, sel: Option<u32>) -> Scene {
        let children = (0..rows)
            .map(|i| {
                let fill = if Some(i) == sel {
                    Color::rgb(0, 0, 255)
                } else {
                    Color::rgb(255, 255, 255)
                };
                Scene::Container(
                    ContainerNode::new(vec![
                        Scene::Box(BoxNode::filled(Rect::new(0, i * 20, 400, 20), fill)),
                        Scene::Text(TextNode::new(
                            format!("Row label {i}"),
                            Rect::new(4, i * 20, 300, 20),
                        )),
                    ])
                    .with_tag(format!("row_{i}")),
                )
            })
            .collect();
        Scene::Container(ContainerNode::new(children).with_tag("grid_root"))
    }

    /// The frame this round exists for: a §5.16 fragment-cache MISS that is
    /// not a §5.36 shape-cache miss.
    ///
    /// Moving a selection changes one row's fill, so that row and its root
    /// re-encode — every glyph in them goes back into the Vello stream — while
    /// not one string changed. Before R1531 that frame re-walked parley for
    /// every text leaf it re-encoded, at 3.1x the cost of the encode it fed
    /// (measured, 1,200 leaves: 1,489 µs -> 480 µs).
    ///
    /// `shapes` alone cannot state this: it was already still across such a
    /// frame, which is precisely why the cost was invisible.
    #[test]
    fn r1531_a_reencoding_frame_rebuilds_no_draw_list() {
        let (mut frag, mut text) = (FragmentCache::new(), LayoutCache::new());
        paint_frame(&text_row_grid(8, None), &mut frag, &mut text);
        let (shapes, builds) = (text.shapes(), text.run_builds());
        assert_eq!(
            (shapes, builds),
            (8, 8),
            "premise: eight distinct labels, each shaped once and derived once",
        );

        // Move the selection down the grid. Every frame misses on the changed
        // row and on the root above it; not one label changed.
        for sel in 0..8 {
            let (_, misses, _) = paint_frame(&text_row_grid(8, Some(sel)), &mut frag, &mut text);
            assert!(misses > 0, "premise: frame {sel} really did re-encode");
        }
        assert_eq!(
            text.run_builds(),
            builds,
            "eight re-encoding frames rebuilt no draw list — the walk is a \
             function of the layout, and no layout changed",
        );
        assert_eq!(text.shapes(), shapes, "and nothing re-shaped either");
    }

    /// The counter reaches the wire unchanged, so an agent reading
    /// `scene/text_cache_stats` sees what the in-process assertion above sees.
    #[test]
    fn r1531_the_snapshot_carries_the_derivation_count() {
        let (mut frag, mut text) = (FragmentCache::new(), LayoutCache::new());
        paint_frame(&text_row_grid(3, None), &mut frag, &mut text);
        assert_eq!(text.stats().run_builds, text.run_builds());
        assert_eq!(text.stats().run_builds, 3);
    }

    /// R1527.1 — the idle fast path skips the trace on a frame that
    /// installed nothing and consulted what the last frame consulted.
    /// This is the frame that breaks the naive version of that rule: it
    /// installs nothing either, but consults something DIFFERENT, and a
    /// sweep is owed. Gating on "zero misses" alone would keep the whole
    /// old tree alive forever.
    #[test]
    fn r1527_a_frame_that_misses_nothing_can_still_owe_a_sweep() {
        let grid = row_grid(4, None);
        let (mut frag, mut text) = (FragmentCache::new(), LayoutCache::new());
        paint_frame(&grid, &mut frag, &mut text);
        let (_, _, entries) = paint_frame(&grid, &mut frag, &mut text);
        assert_eq!(entries, 5, "root + 4 rows");

        // Paint ONE of the rows as the whole scene. Its content is
        // unchanged, so its hash hits — a frame with zero misses whose
        // consulted set is nonetheless `{row_0}` rather than `{root}`.
        let lone_row = grid_row(0, None);
        let (hits, misses, entries) = paint_frame(&lone_row, &mut frag, &mut text);
        assert_eq!(
            (hits, misses),
            (1, 0),
            "the row's content is unchanged, so it hits and nothing encodes"
        );
        assert_eq!(
            entries, 1,
            "the root and the other three rows are painted by nothing now, \
             so they go — a zero-miss frame still owes this sweep"
        );
    }

    /// The trace is transitive and crosses non-cacheable nodes. A
    /// `Scroll` between the root and the rows is not a cache boundary,
    /// so the rows attribute to the nearest cacheable *container*
    /// above them — and a hit on the outermost one still reaches the
    /// innermost fragment two levels down.
    #[test]
    fn r1527_the_trace_is_transitive_through_a_scroll() {
        use pinion_core::scene::ScrollNode;
        let nested = |sel: Option<u32>| {
            Scene::Container(
                ContainerNode::new(vec![Scene::Scroll(ScrollNode::new(
                    Rect::new(0, 0, 400, 100),
                    row_grid(20, sel),
                ))])
                .with_tag("outer"),
            )
        };
        let (mut frag, mut text) = (FragmentCache::new(), LayoutCache::new());

        let (_, misses, entries) = paint_frame(&nested(None), &mut frag, &mut text);
        assert_eq!(misses, 22, "outer + grid_root + 20 rows");
        assert_eq!(entries, 22);

        let (hits, _, entries) = paint_frame(&nested(None), &mut frag, &mut text);
        assert_eq!(hits, 1, "the outermost container answers alone");
        assert_eq!(
            entries, 22,
            "one hit two levels above the rows keeps all 22 alive"
        );

        // And the reuse is real, not just retention: only the root
        // chain and the one changed row re-encode.
        let (hits, misses, _) = paint_frame(&nested(Some(3)), &mut frag, &mut text);
        assert_eq!(
            (hits, misses),
            (19, 3),
            "outer + grid_root + row 3 encode; the other 19 rows replay"
        );
    }

    #[test]
    fn r721_path_arm_paints_fill_stroke_and_curve_no_panic() {
        use pinion_core::scene::{PathCommand, PathNode, PathPoint};
        use pinion_core::style::{PathStyle, Stroke, StrokeCap};

        let p = |x: f32, y: f32| PathPoint::new(x, y);
        // A closed filled triangle, a stroked (round-cap) chevron, and
        // a cubic-Bezier arc — every PathCommand variant + both
        // PathStyle arms (fill / stroke), plus a combined fill+stroke.
        // R1358 — each shape's commands are relative to its OWN rect, so
        // all three are authored in the same 0..60 box and their rects
        // would place them side by side. This test asserts only that the
        // arm does not panic; the placement itself is pinned by pixels in
        // `pinion_shell`'s `r1358_path_commands_paint_relative_to_the_nodes_rect`
        // (the `--ignored` lavapipe job).
        let tri = Scene::Path(PathNode::new(
            Rect::new(0, 0, 60, 60),
            vec![
                PathCommand::MoveTo(p(0.0, 60.0)),
                PathCommand::LineTo(p(60.0, 60.0)),
                PathCommand::LineTo(p(30.0, 0.0)),
                PathCommand::Close,
            ],
            PathStyle::filled(Color::rgb(0x21, 0x96, 0xf3)),
        ));
        let chevron = Scene::Path(PathNode::new(
            Rect::new(60, 0, 60, 60),
            vec![
                PathCommand::MoveTo(p(0.0, 60.0)),
                PathCommand::LineTo(p(30.0, 0.0)),
                PathCommand::LineTo(p(60.0, 60.0)),
            ],
            PathStyle::stroked(
                Stroke::new(Color::rgb(0, 0x96, 0x88), 8).with_cap(StrokeCap::Round),
            ),
        ));
        let arc = Scene::Path(PathNode::new(
            Rect::new(120, 0, 60, 60),
            vec![
                PathCommand::MoveTo(p(0.0, 60.0)),
                PathCommand::CurveTo {
                    c1: p(0.0, 0.0),
                    c2: p(60.0, 0.0),
                    end: p(60.0, 60.0),
                },
            ],
            PathStyle::filled(Color::rgb(0xe5, 0x39, 0x35))
                .with_stroke(Stroke::new(Color::rgb(0x10, 0x10, 0x10), 4)),
        ));
        // An empty command stream paints nothing (early-return no-op).
        let empty = Scene::Path(PathNode::empty(Rect::new(0, 0, 10, 10)));
        let scene = Scene::Container(ContainerNode::new(vec![tri, chevron, arc, empty]));
        let mut text = LayoutCache::new();

        // to_vello (uncached) walker.
        let mut uncached = VelloScene::new();
        to_vello(&scene, &null_hook(), &mut text, &mut uncached);

        // to_vello_cached walker — the path leaf shares paint_path with
        // to_vello, so both arms must reach it without panic.
        let mut cache = FragmentCache::new();
        let mut cached = VelloScene::new();
        to_vello_cached(
            &scene,
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut cached,
        );
        assert_eq!(cache.misses(), 1);
    }

    #[test]
    fn r682_root_effect_or_external_does_not_install_fragment() {
        // The top-level scene is not always a Container.
        // Effect / External roots: no Container boundary to probe at,
        // cache stays empty.
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        let mut v = VelloScene::new();
        to_vello_cached(
            &Scene::Effect(EffectNode::new()),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert_eq!(cache.entries(), 0);

        let mut v = VelloScene::new();
        to_vello_cached(
            &Scene::External(ExternalNode::new(Box::new(
                pinion_core::external::StubExternal::new(),
            ))),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert_eq!(cache.entries(), 0);
    }

    #[test]
    fn r682_three_paint_steady_state_hit_rate_approaches_one() {
        // After warmup (first paint misses + installs), every
        // subsequent identical paint is a hit. After N paints,
        // hit_rate → (N-1)/N.
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        for _ in 0..5 {
            let mut v = VelloScene::new();
            to_vello_cached(
                &simple_container(),
                &null_hook(),
                &mut text,
                &mut ImageCache::new(),
                &mut cache,
                &mut v,
            );
        }
        assert_eq!(cache.hits(), 4);
        assert_eq!(cache.misses(), 1);
        // 4/5 = 0.8.
        assert!((cache.hit_rate() - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn r682_fragment_cache_debug_format_does_not_panic() {
        // Debug impl bypasses VelloScene (which is not Debug) and
        // surfaces just the counters.
        let cache = FragmentCache::new();
        let s = format!("{cache:?}");
        assert!(s.contains("FragmentCache"));
        assert!(s.contains("entries"));
        assert!(s.contains("hits"));
        assert!(s.contains("misses"));
    }

    // ─────────────────────────────────────────────────────────────
    // R682 §5.16 atomic 2 — damage region tracking
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r682_rect_union_of_disjoint_rects_covers_both() {
        let a = Rect::new(0, 0, 50, 50);
        let b = Rect::new(100, 100, 50, 50);
        let u = a.union(b);
        assert_eq!(u, Rect::new(0, 0, 150, 150));
    }

    #[test]
    fn r682_rect_union_of_overlapping_rects_takes_outer_bounds() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        let u = a.union(b);
        assert_eq!(u, Rect::new(0, 0, 150, 150));
    }

    #[test]
    fn r682_rect_union_of_nested_rect_returns_outer() {
        let outer = Rect::new(0, 0, 200, 200);
        let inner = Rect::new(20, 30, 50, 50);
        assert_eq!(outer.union(inner), outer);
        assert_eq!(inner.union(outer), outer);
    }

    #[test]
    fn r682_rect_union_with_zero_area_returns_other() {
        let a = Rect::new(10, 20, 30, 40);
        let zero = Rect::new(0, 0, 0, 0);
        assert_eq!(a.union(zero), a);
        assert_eq!(zero.union(a), a);
    }

    #[test]
    fn r682_damage_region_starts_none() {
        let cache = FragmentCache::new();
        assert!(cache.last_damage_region().is_none());
    }

    #[test]
    fn r682_damage_region_is_miss_rect_after_first_paint() {
        let scene = simple_container();
        // simple_container's root rect is Rect::default = (0,0,0,0).
        // The miss installs a fragment with that root rect; the union
        // accumulator captures it. Match against the root container's
        // actual rect (zero rect contributes the zero-rect-at-(0,0)
        // shape per Rect::union semantics — `Some(Rect(0,0,0,0))`).
        let Scene::Container(ref c) = scene else {
            unreachable!()
        };
        let expected_rect = c.rect;

        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        let mut v = VelloScene::new();
        to_vello_cached(
            &scene,
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );

        let dmg = cache.last_damage_region().expect("first paint = damage");
        assert_eq!(dmg, expected_rect);
    }

    #[test]
    fn r682_damage_region_is_none_after_full_cache_hit_paint() {
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        // First paint: miss.
        let mut v = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert!(cache.last_damage_region().is_some(), "first paint dirtied");
        // Second paint: same scene → cache hit → no damage published.
        let mut v = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert!(
            cache.last_damage_region().is_none(),
            "100% cache-hit paint = no damage region"
        );
    }

    #[test]
    fn r682_damage_region_unions_multiple_misses() {
        // Two sibling cacheable Container children with different
        // non-zero rects: both miss on first paint, damage region is
        // their union.
        let mut first = ContainerNode::new(vec![Scene::Box(BoxNode::filled(
            Rect::new(10, 10, 40, 20),
            Color::default(),
        ))]);
        first.rect = Rect::new(10, 10, 40, 20);

        let mut second = ContainerNode::new(vec![Scene::Box(BoxNode::filled(
            Rect::new(200, 200, 50, 50),
            Color::default(),
        ))]);
        second.rect = Rect::new(200, 200, 50, 50);

        // Root is uncacheable so children miss individually instead
        // of getting absorbed into a root fragment. Use a fixed-rect
        // root wrapping the children via an ImmediateMode sentinel
        // that prevents root caching but doesn't itself miss-publish.
        let root_uncacheable = Scene::Container(ContainerNode::new(vec![
            Scene::Container(first),
            Scene::Container(second),
            Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
                InertImmediate,
                Rect::new(0, 0, 1, 1),
            )),
        ]));

        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        let mut v = VelloScene::new();
        to_vello_cached(
            &root_uncacheable,
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );

        assert_eq!(cache.misses(), 2, "both children miss");
        let dmg = cache.last_damage_region().expect("two misses = damage");
        // Union of (10,10,40,20) and (200,200,50,50) =
        // (10, 10, 240, 240) — bounding box covers both.
        assert_eq!(dmg, Rect::new(10, 10, 240, 240));
    }

    #[test]
    fn r682_damage_region_resets_between_paints() {
        // First paint accumulates damage; second paint (all hits)
        // publishes None.
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();

        let mut v = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert!(cache.last_damage_region().is_some());

        // Second identical paint: all hits, damage publishes None
        // (not the previous paint's damage).
        let mut v = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert!(cache.last_damage_region().is_none());

        // Third paint with mutated scene: miss, damage republishes.
        let mut v = VelloScene::new();
        to_vello_cached(
            &mutated_container(),
            &null_hook(),
            &mut text,
            &mut ImageCache::new(),
            &mut cache,
            &mut v,
        );
        assert!(cache.last_damage_region().is_some());
    }

    // ─────────────────────────────────────────────────────────────
    // R682.B §5.16 — paint-fragment cache stress consumer matrix
    // ─────────────────────────────────────────────────────────────
    //
    // The R682.B 100-row stress consumer (`examples/todomvc` with
    // [`SEED_N_ENV`]) exercises the cache at a scale closer to a
    // real DCC application. The tests below pin the matrix on a
    // synthetic N-row scene so the substrate's contract holds
    // independently of the binding's view-fn shape.
    //
    // ## Scene shape — non-cacheable-root + cacheable-rows
    //
    // The stress consumer places an `ImmediateModeNode` sibling
    // alongside the row list so the enclosing root Container is
    // **not** cacheable per
    // [`Scene::is_cacheable_for_paint`] (any
    // immediate-mode / external descendant taints the ancestor).
    // The cache-probe early-return at the root short-circuit boundary
    // (`to_vello_cached_inner` line ~565) does not fire; the encoder
    // walks past the non-cacheable root and reaches each row
    // Container independently. Each row IS cacheable (Box-only leaf),
    // so each row probes the cache → independent hit/miss + survival
    // across mark-and-sweep.
    //
    // This shape matches the real todomvc + every DCC-class binding:
    // the page-level chrome (header / filters / scroll handles /
    // immediate-mode preview) is naturally non-cacheable, so the
    // per-row cache lives at the row Container boundary. The
    // alternative ("fully cacheable root") collapses to 1 cached
    // entry after the first sweep (hit on root → no recursion into
    // children → children evicted) — observably useful only when the
    // **entire** subtree never changes for the binding's lifetime
    // (rare).

    /// R682.B §5.16 — Build a synthetic stress scene with N
    /// cacheable row Containers under a non-cacheable root.
    ///
    /// Layout: `Container[stress_root]([ImmediateModeNode,
    /// Container[row_0](Box), Container[row_1](Box), …])`. The
    /// `ImmediateModeNode` sibling makes the root non-cacheable so
    /// the encoder walks past it and visits each row Container.
    ///
    /// `completed_mask[i] == true` paints row `i` with a green-channel
    /// fill (model "completed" status); `false` paints a red-channel
    /// fill. Distinct colour per row → distinct `paint_hash` per row
    /// → independent cache slots (no deduplication).
    fn stress_scene(n: u32, completed_mask: &[bool]) -> Scene {
        assert_eq!(completed_mask.len(), n as usize);
        let mut children: Vec<Scene> = Vec::with_capacity(n as usize + 1);
        // Non-cacheable sibling at the head of the children list so
        // the root container's `is_cacheable_for_paint` returns false
        // and the encoder threads past the root → reaches each row.
        children.push(Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            InertImmediate,
            Rect::new(0, 0, 100, 1),
        )));
        for i in 0..n {
            #[allow(clippy::cast_possible_truncation, reason = "color byte from index")]
            let g = (i % 256) as u8;
            let row_fill = if completed_mask[i as usize] {
                Color::rgb(0, g, 0)
            } else {
                Color::rgb(g, 0, 0)
            };
            let row = Scene::Container(
                ContainerNode::new(vec![Scene::Box(BoxNode::filled(
                    Rect::new(0, i * 10 + 10, 100, 10),
                    row_fill,
                ))])
                .with_tag(format!("row_{i}")),
            );
            children.push(row);
        }
        Scene::Container(ContainerNode::new(children).with_tag("stress_root"))
    }

    fn run_paint(scene: &Scene, cache: &mut FragmentCache, text: &mut LayoutCache) {
        let mut v = VelloScene::new();
        to_vello_cached(
            scene,
            &null_hook(),
            text,
            &mut ImageCache::new(),
            cache,
            &mut v,
        );
    }

    #[test]
    fn r682b_stress_first_paint_installs_n_row_fragments() {
        // 100 cacheable rows install on first paint. Root is
        // non-cacheable (`ImmediateModeNode` sibling) → no root entry
        // in the cache. Total entries = N (NOT N+1).
        let mask = vec![false; 100];
        let scene = stress_scene(100, &mask);
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        run_paint(&scene, &mut cache, &mut text);
        assert_eq!(cache.misses(), 100, "100 cacheable rows miss first paint");
        assert_eq!(cache.hits(), 0);
        assert_eq!(
            cache.entries(),
            100,
            "row fragments installed; root untracked"
        );
    }

    #[test]
    fn r682b_stress_steady_state_hit_rate_clears_threshold_after_7_paints() {
        // After P identical paints with N cacheable rows:
        //   misses = N (paint 1 only) ; hits = (P-1)*N
        //   hit_rate = (P-1)/P
        // For hit_rate ≥ 0.85: P ≥ 7. The R682.B authoritative
        // threshold — long-running steady state, NOT 3 paints. The
        // SEED `≥0.85 after 3 paints` figure was an authorship
        // estimate that did not account for the (P-1)/P ramp; the
        // honest correction lands here.
        let mask = vec![false; 100];
        let scene = stress_scene(100, &mask);
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        for _ in 0..7 {
            run_paint(&scene, &mut cache, &mut text);
        }
        assert_eq!(cache.misses(), 100);
        assert_eq!(cache.hits(), 600, "6 hit-paints × 100 rows");
        assert!(
            cache.hit_rate() >= 0.85,
            "post-warmup hit rate {} must clear 0.85 by paint 7",
            cache.hit_rate(),
        );
        assert_eq!(cache.paint_count(), 7);
        // Per-row entries survive across paints — sweep keeps every
        // hash that was consulted.
        assert_eq!(cache.entries(), 100);
    }

    #[test]
    fn r682b_stress_filter_change_evicts_filtered_out_row_fragments() {
        // Paint 1: full 100-row scene (100 row misses, 100 entries).
        // Paint 2: filter to the 34 completed rows; the encoder
        // visits only those rows. The 66 active-row hashes are not
        // consulted → mark-and-sweep evicts them at end_paint.
        let mut full_mask = vec![false; 100];
        for (i, slot) in full_mask.iter_mut().enumerate() {
            *slot = i % 3 == 0;
        }
        let full = stress_scene(100, &full_mask);

        // Build a filtered scene that contains only the completed
        // rows (their fragments will survive). Tags + colours +
        // rects match the originals so the hashes match and the
        // cache hits.
        let completed_indices: Vec<u32> = (0..100u32).filter(|i| full_mask[*i as usize]).collect();
        let completed_n = u32::try_from(completed_indices.len()).expect("≤ 100 fits u32");
        let mut filtered_children: Vec<Scene> = vec![Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(InertImmediate, Rect::new(0, 0, 100, 1)),
        )];
        for i in &completed_indices {
            #[allow(clippy::cast_possible_truncation, reason = "i < 100 fits in u8")]
            let g = (*i % 256) as u8;
            filtered_children.push(Scene::Container(
                ContainerNode::new(vec![Scene::Box(BoxNode::filled(
                    Rect::new(0, i * 10 + 10, 100, 10),
                    Color::rgb(0, g, 0),
                ))])
                .with_tag(format!("row_{i}")),
            ));
        }
        let filtered =
            Scene::Container(ContainerNode::new(filtered_children).with_tag("stress_root"));

        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();

        run_paint(&full, &mut cache, &mut text);
        assert_eq!(cache.entries(), 100, "warmup populates 100 row entries");

        run_paint(&filtered, &mut cache, &mut text);
        // After paint 2:
        // - 34 surviving completed-row entries (consulted via hits).
        // - 66 active-row entries evicted (not consulted).
        assert_eq!(
            cache.entries(),
            completed_n as usize,
            "filter change evicts dropped-row fragments; only survivors remain",
        );
        // Hits on the 34 surviving completed rows.
        assert_eq!(
            cache.hits(),
            u64::from(completed_n),
            "every still-visible row hits the previously installed fragment",
        );
        // No new row misses on paint 2 (every visible row was
        // installed during warmup).
        assert_eq!(cache.misses(), 100);
    }

    #[test]
    fn r682b_stress_full_hit_paint_publishes_no_damage_region() {
        // First paint dirties every row (N misses); second identical
        // paint hits every row (no miss) → no damage rect
        // accumulator entries → `last_damage_region == None`.
        let mask = vec![false; 50];
        let scene = stress_scene(50, &mask);
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        run_paint(&scene, &mut cache, &mut text);
        assert!(
            cache.last_damage_region().is_some(),
            "first paint dirties every row",
        );
        run_paint(&scene, &mut cache, &mut text);
        assert!(
            cache.last_damage_region().is_none(),
            "100% hit paint publishes no damage",
        );
    }

    #[test]
    fn r682b_stress_stats_snapshot_reflects_live_counters() {
        // Exercise the GUI-agnostic snapshot path consumers (RPC +
        // tests) actually use: `FragmentCache::stats()`. The
        // counters surfaced through the value-type
        // [`FragmentCacheStats`] must match the direct getters.
        let mask = vec![false; 100];
        let scene = stress_scene(100, &mask);
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        for _ in 0..3 {
            run_paint(&scene, &mut cache, &mut text);
        }
        let stats = cache.stats();
        assert_eq!(stats.hits, cache.hits());
        assert_eq!(stats.misses, cache.misses());
        assert_eq!(stats.paint_count, cache.paint_count());
        assert_eq!(stats.entries, cache.entries());
        // last_damage_region: third stable paint must be all-hit →
        // None.
        assert!(stats.last_damage_region.is_none());
        assert!((stats.hit_rate() - cache.hit_rate()).abs() < f32::EPSILON);
    }

    #[test]
    fn r682b_stress_paint_count_advances_monotonically() {
        // Each `to_vello_cached` invocation increments paint_count by
        // exactly one. R682 demo asserts this against the RPC
        // surface; here we pin the substrate-level contract.
        let scene = stress_scene(10, &[false; 10]);
        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        for expected_count in 1u64..=5 {
            run_paint(&scene, &mut cache, &mut text);
            assert_eq!(cache.paint_count(), expected_count);
        }
    }

    #[test]
    fn r1001_fit_font_size_to_cell_reduces_overflowing_box() {
        // A natural line box larger than the cell (the descender-clip cause)
        // is reduced so the *scaled* box fits — font-independent policy math.
        let cell_h = 16u32;
        let natural = 19.2_f64; // 1.2 × 16, a typical monospace line box
        let fitted = fit_font_size_to_cell(natural, cell_h);
        assert!(
            fitted < cell_h,
            "an overflowing box must shrink the font: {fitted}"
        );
        // The line box scales linearly with font size, so the fitted box is
        // `natural × fitted / cell_h` — and it must fit the cell.
        let fitted_box = natural * f64::from(fitted) / f64::from(cell_h);
        assert!(
            fitted_box <= f64::from(cell_h),
            "fitted box {fitted_box} must fit cell {cell_h}",
        );
        assert_eq!(fitted, 13, "floor(16²/19.2) = floor(13.33)");
    }

    #[test]
    fn r1001_fit_font_size_to_cell_keeps_fitting_box() {
        // A box already within the cell needs no reduction.
        assert_eq!(fit_font_size_to_cell(16.0, 16), 16);
        assert_eq!(fit_font_size_to_cell(15.0, 16), 16);
    }

    #[test]
    fn r1001_fit_font_size_to_cell_guards_degenerate_box() {
        // Non-finite / zero measurements fall back to the cell height; an
        // extreme box still clamps to a >= 1 font size (never zero / panic).
        assert_eq!(fit_font_size_to_cell(f64::NAN, 16), 16);
        assert_eq!(fit_font_size_to_cell(0.0, 16), 16);
        assert!(fit_font_size_to_cell(1_000.0, 16) >= 1);
    }

    /// R1178 §5.41 — FULL BLOCK fills the whole cell, and two horizontally
    /// adjacent cells abut on an exact pixel boundary: cell N's right edge
    /// equals cell N+1's left edge, so a run of `█` tiles with no gap (the
    /// reported "broken bars" symptom). This is the geometry behind the GPU
    /// `r1178_block_element_full_block_tiles_without_gap` pixel guard.
    #[test]
    fn r1178_full_block_fills_cell_and_tiles_gap_free() {
        let (a, na) =
            block_element_rects('\u{2588}', KurboRect::new(0.0, 0.0, 10.0, 30.0)).expect("block");
        assert_eq!(na, 1);
        assert_eq!((a[0].x0, a[0].y0, a[0].x1, a[0].y1), (0.0, 0.0, 10.0, 30.0));
        // The next cell to the right (cx = 10) starts exactly where this ends.
        let (b, _) =
            block_element_rects('\u{2588}', KurboRect::new(10.0, 0.0, 20.0, 30.0)).expect("block");
        assert!(
            (a[0].x1 - b[0].x0).abs() < f64::EPSILON,
            "adjacent full blocks must share an edge: {} vs {}",
            a[0].x1,
            b[0].x0,
        );
    }

    /// R1178 §5.41 — halves split the cell at its integer-snapped midpoint, so
    /// a half block exactly covers its fraction (no fitted-glyph margin).
    #[test]
    fn r1178_half_blocks_cover_exact_fraction() {
        let cell = |c: char| {
            block_element_rects(c, KurboRect::new(0.0, 0.0, 10.0, 30.0))
                .expect("block")
                .0[0]
        };
        // cell_w/2 = 5, cell_h/2 = 15.
        let upper = cell('\u{2580}'); // upper half
        assert_eq!((upper.y0, upper.y1), (0.0, 15.0));
        let lower = cell('\u{2584}'); // lower half
        assert_eq!((lower.y0, lower.y1), (15.0, 30.0));
        let left = cell('\u{258C}'); // left half
        assert_eq!((left.x0, left.x1), (0.0, 5.0));
        let right = cell('\u{2590}'); // right half
        assert_eq!((right.x0, right.x1), (5.0, 10.0));
    }

    /// R1178 §5.41 — quadrant combos emit one fill per filled quadrant, all
    /// meeting at the integer-snapped cell centre (no interior AA seam).
    #[test]
    fn r1178_quadrant_combo_emits_meeting_rects() {
        let cell = KurboRect::new(0.0, 0.0, 10.0, 30.0);
        // U+2598 ▘ — a single upper-left quadrant.
        let (ul, n_ul) = block_element_rects('\u{2598}', cell).expect("quadrant");
        assert_eq!(n_ul, 1);
        assert_eq!(
            (ul[0].x0, ul[0].y0, ul[0].x1, ul[0].y1),
            (0.0, 0.0, 5.0, 15.0)
        );
        // U+2599 ▙ — upper-left + lower-left + lower-right (three quadrants).
        let (tri, n_tri) = block_element_rects('\u{2599}', cell).expect("quadrant");
        assert_eq!(n_tri, 3);
        assert_eq!(
            (tri[0].x0, tri[0].y0, tri[0].x1, tri[0].y1),
            (0.0, 0.0, 5.0, 15.0)
        ); // UL
        assert_eq!(
            (tri[1].x0, tri[1].y0, tri[1].x1, tri[1].y1),
            (0.0, 15.0, 5.0, 30.0)
        ); // LL
        assert_eq!(
            (tri[2].x0, tri[2].y0, tri[2].x1, tri[2].y1),
            (5.0, 15.0, 10.0, 30.0)
        ); // LR
    }

    /// R1178 §5.41 — non-block codepoints return `None` and keep the glyph
    /// path: ordinary text, the shade blocks (an alpha pattern, not a solid
    /// fill), and box-drawing.
    #[test]
    fn r1178_non_solid_block_falls_through_to_glyph() {
        let cell = KurboRect::new(0.0, 0.0, 10.0, 30.0);
        for c in [
            'M', ' ', '\u{2591}', '\u{2592}', '\u{2593}', '\u{2500}', '\u{2502}',
        ] {
            assert!(
                block_element_rects(c, cell).is_none(),
                "{c:?} must fall through to the glyph path",
            );
        }
    }

    /// R1181 §5.41 — the SSOT gate: a lone codepoint in `U+2500`–`U+259F` is
    /// synthesizable; out-of-range, multi-char, and empty clusters are not.
    #[test]
    fn r1181_synthesizable_char_gate() {
        // box / block / shade boundaries are all in range.
        for s in [
            "\u{2500}", "\u{257F}", "\u{2580}", "\u{2588}", "\u{2591}", "\u{259F}",
        ] {
            assert!(synthesizable_char(s).is_some(), "{s:?} is in range");
        }
        // just below / just above the range, plain text, multi-char, empty.
        for s in [
            "\u{24FF}",
            "\u{25A0}",
            "M",
            " ",
            "",
            "ab",
            "\u{2588}\u{2588}",
        ] {
            assert_eq!(synthesizable_char(s), None, "{s:?} is not synthesizable");
        }
    }

    /// R1179 §5.41 — the three shade blocks map to 25 / 50 / 75 % ink alpha;
    /// everything else (solid blocks, text, box-drawing) is not a shade.
    #[test]
    fn r1179_shade_block_fractions() {
        assert_eq!(shade_block_fraction('\u{2591}'), Some((1, 4)));
        assert_eq!(shade_block_fraction('\u{2592}'), Some((1, 2)));
        assert_eq!(shade_block_fraction('\u{2593}'), Some((3, 4)));
        for c in ['\u{2588}', '\u{2500}', 'M'] {
            assert_eq!(shade_block_fraction(c), None, "{c:?} is not a shade");
        }
    }

    /// R1180 §5.41 — the box-drawing classifier returns the canonical arm
    /// decomposition (light / heavy / double / dashed) and the rounded / diagonal
    /// specials; non-box codepoints (text, blocks, shades) are `None`.
    #[test]
    fn r1180_box_drawing_classifies_canonical_glyphs() {
        use BoxGlyph::{Arc, Arms, Diagonal};
        let arms = |up, down, left, right, dash| {
            Some(Arms {
                up,
                down,
                left,
                right,
                dash,
            })
        };
        assert_eq!(box_drawing('\u{2500}'), arms(0, 0, 1, 1, 0)); // ─ light
        assert_eq!(box_drawing('\u{253C}'), arms(1, 1, 1, 1, 0)); // ┼ cross
        assert_eq!(box_drawing('\u{250F}'), arms(0, 2, 0, 2, 0)); // ┏ heavy
        assert_eq!(box_drawing('\u{2550}'), arms(0, 0, 3, 3, 0)); // ═ double
        assert_eq!(box_drawing('\u{2504}'), arms(0, 0, 1, 1, 3)); // ┄ triple dash
        assert_eq!(
            box_drawing('\u{256D}'),
            Some(Arc {
                down: true,
                right: true
            })
        ); // ╭
        assert_eq!(
            box_drawing('\u{2573}'),
            Some(Diagonal {
                slash: true,
                backslash: true
            }) // ╳
        );
        for c in ['M', '\u{2588}', '\u{2591}', ' ', '\u{24FF}', '\u{2580}'] {
            assert_eq!(box_drawing(c), None, "{c:?} is not box-drawing");
        }
    }

    /// R1180 §5.41 — arm rectangles cover the right bands and, crucially,
    /// adjacent arms overlap at the cell centre so corners / junctions connect.
    #[test]
    fn r1180_box_arm_rects_geometry_and_connectivity() {
        let cell = KurboRect::new(0.0, 0.0, 16.0, 16.0);
        let span_x = |rs: &[KurboRect]| {
            let lo = rs.iter().map(|r| r.x0).fold(f64::INFINITY, f64::min);
            let hi = rs.iter().map(|r| r.x1).fold(f64::NEG_INFINITY, f64::max);
            (lo, hi)
        };
        let span_y = |rs: &[KurboRect]| {
            let lo = rs.iter().map(|r| r.y0).fold(f64::INFINITY, f64::min);
            let hi = rs.iter().map(|r| r.y1).fold(f64::NEG_INFINITY, f64::max);
            (lo, hi)
        };
        let count =
            |up, down, left, right, dash| box_arm_rects(up, down, left, right, dash, cell).1;
        // ─ light horizontal: left + right half-arms abutting at the centre,
        // together spanning the full width at mid-height.
        let (hr, hn) = box_arm_rects(0, 0, 1, 1, 0, cell);
        let h = &hr[..hn];
        assert_eq!(hn, 2);
        assert!(
            (h[0].x1 - h[1].x0).abs() < 1e-9,
            "halves abut at the centre"
        );
        let (lo, hi) = span_x(h);
        assert!(
            (lo - 0.0).abs() < 1e-9 && (hi - 16.0).abs() < 1e-9,
            "spans full width"
        );
        assert!(h[0].y0 < 8.0 && h[0].y1 > 8.0, "band straddles centre y");
        // │ light vertical: up + down half-arms spanning the full height.
        let (vr, vn) = box_arm_rects(1, 1, 0, 0, 0, cell);
        assert_eq!(vn, 2);
        let (lo, hi) = span_y(&vr[..vn]);
        assert!(
            (lo - 0.0).abs() < 1e-9 && (hi - 16.0).abs() < 1e-9,
            "spans full height"
        );
        // ┌ (down+right): the two arms both cover the centre — a connected corner.
        let (cr, cn) = box_arm_rects(0, 1, 0, 1, 0, cell);
        assert_eq!(cn, 2);
        let covers =
            |r: &KurboRect, x: f64, y: f64| r.x0 <= x && x <= r.x1 && r.y0 <= y && y <= r.y1;
        assert_eq!(
            cr[..cn].iter().filter(|r| covers(r, 8.0, 8.0)).count(),
            2,
            "both arms must meet at the cell centre",
        );
        // ═ double: two parallel rails per horizontal arm (left + right) => 4.
        assert_eq!(count(0, 0, 3, 3, 0), 4);
        // ╬ all-arms double: 2 rails × 4 arms = the 8-rect maximum.
        assert_eq!(count(3, 3, 3, 3, 0), 8);
        // ┄ triple dash: three dash segments.
        assert_eq!(count(0, 0, 1, 1, 3), 3);
    }
}

/// R1550 §5.16 §5.7 — what an encoded fragment is holding, in bytes.
///
/// A `vello::Scene` is a command stream, and `Scene::encoding()` hands out the
/// whole of it: eight `Vec`s of plain data plus a `Resources` of five more.
/// Every one reports its own capacity, so this arena is measured **to the
/// byte** — no foreign interior is hidden behind a private field the way
/// parley's `Layout` hides its glyph buffers.
///
/// The count that matters here is capacity, not length. A fragment encoded for
/// a large container and then re-encoded smaller keeps the larger buffer, and
/// the buffer is what the process is holding.
#[must_use]
pub fn encoded_scene_bytes(scene: &VelloScene) -> usize {
    let enc = scene.encoding();
    let res = &enc.resources;
    // `capacity * size_of::<element>()` per stream. `size_of_val` over the
    // slice would price the LENGTH, and a buffer that shrank still holds what
    // it grew to.
    let stream = |cap: usize, elem: usize| cap.saturating_mul(elem);
    stream(
        enc.path_tags.capacity(),
        size_of_val_of_first(&enc.path_tags),
    )
    .saturating_add(stream(enc.path_data.capacity(), size_of::<u32>()))
    .saturating_add(stream(
        enc.draw_tags.capacity(),
        size_of_val_of_first(&enc.draw_tags),
    ))
    .saturating_add(stream(enc.draw_data.capacity(), size_of::<u32>()))
    .saturating_add(stream(
        enc.transforms.capacity(),
        size_of_val_of_first(&enc.transforms),
    ))
    .saturating_add(stream(
        enc.styles.capacity(),
        size_of_val_of_first(&enc.styles),
    ))
    .saturating_add(stream(
        res.patches.capacity(),
        size_of_val_of_first(&res.patches),
    ))
    .saturating_add(stream(
        res.color_stops.capacity(),
        size_of_val_of_first(&res.color_stops),
    ))
    .saturating_add(stream(
        res.glyphs.capacity(),
        size_of_val_of_first(&res.glyphs),
    ))
    .saturating_add(stream(
        res.glyph_runs.capacity(),
        size_of_val_of_first(&res.glyph_runs),
    ))
    .saturating_add(stream(
        res.normalized_coords.capacity(),
        size_of_val_of_first(&res.normalized_coords),
    ))
}

/// The element size of a `Vec` whose element type is not nameable here.
///
/// Several `vello_encoding` stream element types (`PathTag`, `DrawTag`,
/// `Style`, `Patch`, `GlyphRun`) are public values in types this crate cannot
/// name through vello's re-export surface. `size_of` over the slice's element
/// type is reachable generically, which is all the arithmetic needs.
fn size_of_val_of_first<T>(_v: &[T]) -> usize {
    size_of::<T>()
}

/// R1550 §5.16 §5.7 — the §5.16 paint fragment arena's accounting.
///
/// Exact: every value here is either plain data or a `vello::Scene`, and
/// [`encoded_scene_bytes`] sizes those completely.
mod footprint {
    use super::{FragmentCache, encoded_scene_bytes};
    use pinion_core::footprint::Footprint;
    use pinion_core::memory_census::{Arena, ArenaFootprint, MeasuredArena};

    impl Footprint for FragmentCache {
        fn footprint(&self) -> usize {
            let Self {
                fragments,
                seen_this_paint,
                subsumes,
                child_stack,
                seen_last_paint,
                misses_at_begin,
                damage_acc_this_paint,
                last_damage_region,
                hits,
                misses,
                paint_count,
                nodes_this_paint,
                nodes_last_paint,
            } = self;
            // `fragments` is a `HashMap<u64, VelloScene>` and a `VelloScene`
            // is foreign, so the map's own blanket impl cannot reach it; the
            // table is priced here and its values through the accessor.
            pinion_core::footprint::hash_table_bytes::<(u64, super::VelloScene)>(
                fragments.capacity(),
            )
            .saturating_add(fragments.values().map(encoded_scene_bytes).sum::<usize>())
                + seen_this_paint.footprint()
                + subsumes.footprint()
                + child_stack.footprint()
                + seen_last_paint.footprint()
                + misses_at_begin.footprint()
                + damage_acc_this_paint.footprint()
                + last_damage_region.footprint()
                + hits.footprint()
                + misses.footprint()
                + paint_count.footprint()
                + nodes_this_paint.footprint()
                + nodes_last_paint.footprint()
        }
    }

    impl MeasuredArena for FragmentCache {
        fn arena_footprint(&self) -> ArenaFootprint {
            ArenaFootprint::exact(
                Arena::PaintFragments,
                self.footprint() as u64,
                self.entries() as u64,
            )
        }
    }
}
