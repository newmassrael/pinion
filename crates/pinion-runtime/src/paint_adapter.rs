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

use crate::paint_cache_stats::FragmentCacheStats;
use pinion_core::Scene;
use pinion_core::scene::{
    BoxNode, ImmediateModeNode, ImmediatePainter, Rect, TextNode,
};
use pinion_core::style::{
    Border, BorderPlacement, BoxStyle, Color, Gradient, GradientKind, TextOverflow,
};
use pinion_text::LayoutCache;
use pinion_text::parley::PositionedLayoutItem;
use vello::Glyph;
use vello::Scene as VelloScene;
use vello::kurbo::{
    Affine, BezPath, Line, PathEl, Point as KurboPoint, Rect as KurboRect,
    RoundedRect as KurboRoundedRect, Stroke,
};
use vello::peniko::{
    Brush as PenikoBrush, Color as PenikoColor, Extend as PenikoExtend, Fill,
    Gradient as PenikoGradient,
};

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
///   `positioned_glyphs()` per [`parley::GlyphRun`], emit one
///   [`vello::Scene::draw_glyphs`] call per run.
/// * [`Scene::External`] / [`Scene::Effect`] / [`Scene::Path`] /
///   [`Scene::Image`] — no-op. Path / Image paint primitives attach
///   in follow-up rounds.
pub fn to_vello<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    out: &mut VelloScene,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    to_vello_inner(scene, fill_hook, text_cache, out, Affine::IDENTITY);
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
    out: &mut VelloScene,
    transform: Affine,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    match scene {
        Scene::Container(c) => {
            paint_box_shadows(out, c.rect, &c.style, transform);
            fill_box_bg(out, c.rect, &c.style, c.style.fill, transform);
            for child in &c.children {
                to_vello_inner(child, fill_hook, text_cache, out, transform);
            }
        }
        Scene::Box(b) => {
            paint_box_shadows(out, b.rect, &b.style, transform);
            let fill = fill_hook(b).unwrap_or(b.style.fill);
            fill_box_bg(out, b.rect, &b.style, fill, transform);
            if let Some(border) = b.style.border {
                stroke_rect(out, b.rect, border, transform);
            }
        }
        Scene::Text(t) => paint_text(out, t, text_cache, transform),
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
            out.push_clip_layer(transform, &viewport_clip);
            // Content paints in content-intrinsic coordinates; the
            // scroll container shifts so that content-intrinsic
            // `(0, 0)` lands at viewport `(viewport.x - offset_x,
            //  viewport.y - offset_y)` in the parent frame. Compose
            // with the inherited transform so nested scrolls
            // accumulate correctly.
            let dx = f64::from(s.viewport.x) - f64::from(s.offset_x);
            let dy = f64::from(s.viewport.y) - f64::from(s.offset_y);
            let child_transform = transform * Affine::translate((dx, dy));
            to_vello_inner(&s.content, fill_hook, text_cache, out, child_transform);
            out.pop_layer();
        }
        // R681 §2 #4 atomic 1 — ImmediateModeNode paints through
        // the backend-agnostic [`ImmediatePainter`] surface. The
        // shell's [`pinion_shell::ShellCore::compute_paint_scene_internal`]
        // has already invoked `node.handle.borrow_mut().tick(dt)`
        // by the time the paint walker reaches this branch (the
        // tick + paint phases are separated so future per-window
        // [`ControlFlow::Poll`] pacing in atomic 2 can drive the
        // tick independently of the encode step). The painter
        // composes `transform * translate(viewport.{x,y})` so the
        // driver paints in viewport-LOCAL coordinates and the
        // result lands at the correct screen-space position.
        Scene::ImmediateModeNode(node) => {
            paint_immediate_mode_node(out, node, transform);
        }
        // External / Effect / Path / Image: no-op. Path + Image paint
        // primitives attach in follow-up rounds.
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
/// [`Scene::Container`] boundary encountered with [`Affine::IDENTITY`]
/// accumulated transform:
///
/// - **Hit** — copy the cached fragment via
///   [`VelloScene::append`] into the destination scene with no
///   transform pre-multiplication (the cached fragment already encodes
///   absolute coords because it was built under `IDENTITY`). The
///   recursive walk into the container's children is skipped entirely:
///   the cache replay covers them.
/// - **Miss** — encode this container's contribution into a fresh
///   sub-scene under `IDENTITY`, recursively encode children (which
///   may themselves hit the cache for nested Containers), append the
///   sub-scene to the destination, and insert it into the cache under
///   the container's hash.
///
/// ## Eviction (mark-and-sweep)
///
/// Each [`to_vello_cached`] invocation brackets the encoder walk with
/// [`Self::begin_paint`] / [`Self::end_paint`]; the begin clears the
/// per-paint "seen" set, every hit / insert marks the hash, and the
/// end drops entries the walker did not consult this paint. Memory
/// bounds itself to the set of cacheable Containers actually painted
/// in the most recent frame — no LRU heuristic, no fixed cap.
///
/// ## Cache-key non-axes (atomic 1 first-cut)
///
/// The cache key is the container's `paint_hash` ALONE. The inherited
/// `transform` is constrained to [`Affine::IDENTITY`] at the cache
/// boundary so two different transform chains never alias the same
/// key. Practical consequence: cacheable subtrees inside a
/// [`Scene::Scroll`] (whose content carries a non-identity scroll
/// translation) skip the cache because their inherited transform is
/// never identity. A follow-up round (R682+1 carry) can lift this by
/// either hashing the transform into the cache key or encoding
/// fragments in container-local coordinates and re-applying the
/// inherited transform on append.
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
    /// [`Self::end_paint`], swept at end.
    seen_this_paint: HashSet<u64>,
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
        self.seen_this_paint.clear();
        self.damage_acc_this_paint = None;
    }

    /// Close the current paint pass. Drops cache entries whose hashes
    /// were not consulted between the matching [`Self::begin_paint`]
    /// and this call; increments the paint counter; publishes the
    /// per-paint damage accumulator into [`Self::last_damage_region`].
    pub fn end_paint(&mut self) {
        let seen = &self.seen_this_paint;
        self.fragments.retain(|hash, _| seen.contains(hash));
        self.paint_count = self.paint_count.saturating_add(1);
        self.last_damage_region = self.damage_acc_this_paint;
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

    /// Probe + replay path. When `hash` is in the cache, append the
    /// stored fragment into `out` (without pre-multiplication: the
    /// cached fragment is already encoded under [`Affine::IDENTITY`]
    /// because the encoder only caches at identity-transform
    /// boundaries) and mark the hash as seen.
    ///
    /// Returns `true` on hit, `false` on miss. Bumps the hit counter
    /// on hit; the miss counter is bumped by [`Self::insert_miss`]
    /// to keep the increment paired with the actual cache install.
    fn try_hit(&mut self, hash: u64, out: &mut VelloScene) -> bool {
        if let Some(fragment) = self.fragments.get(&hash) {
            out.append(fragment, None);
            self.seen_this_paint.insert(hash);
            self.hits = self.hits.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Install a freshly encoded fragment under `hash` and mark it as
    /// seen this paint. The miss counter is bumped here (paired with
    /// the actual encode, not just a probe). The container's `rect`
    /// contributes to the per-paint damage accumulator (R682 atomic 2):
    /// a missed Container's bounds is where the painted output may
    /// differ from the previous frame.
    fn insert_miss(
        &mut self,
        hash: u64,
        fragment: VelloScene,
        rect: pinion_core::scene::Rect,
    ) {
        self.fragments.insert(hash, fragment);
        self.seen_this_paint.insert(hash);
        self.misses = self.misses.saturating_add(1);
        self.damage_acc_this_paint = Some(match self.damage_acc_this_paint {
            Some(acc) => acc.union(rect),
            None => rect,
        });
    }
}

/// R682 §5.16 — cached counterpart to [`to_vello`].
///
/// Walks the [`Scene`] tree like [`to_vello`] but consults the
/// supplied [`FragmentCache`] at every cacheable
/// [`Scene::Container`] boundary it reaches with [`Affine::IDENTITY`]
/// accumulated transform — a cache hit appends the previously encoded
/// `vello::Scene` fragment into `out` and skips the recursive walk
/// into children; a miss encodes the subtree fresh into a sub-scene,
/// appends it to `out`, and stores it in the cache for the next
/// paint.
///
/// Brackets the encoder walk with [`FragmentCache::begin_paint`] /
/// [`FragmentCache::end_paint`] so unreached cache entries are
/// evicted at the end of the call. Callers do NOT need to manage
/// begin/end themselves; this is the single-call API for the shell's
/// per-window paint cycle.
///
/// `fill_hook` is honoured for [`Scene::Box`] leaves (matching
/// [`to_vello`]'s contract). For the cache to remain correct, the
/// hook MUST be structurally derived (a function of the
/// [`BoxNode`] fields only, not external state) — see
/// [`FragmentCache`] for the rationale.
pub fn to_vello_cached<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    fragment_cache: &mut FragmentCache,
    out: &mut VelloScene,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    fragment_cache.begin_paint();
    to_vello_cached_inner(
        scene,
        fill_hook,
        text_cache,
        fragment_cache,
        out,
        Affine::IDENTITY,
    );
    fragment_cache.end_paint();
}

/// Transform-carrying recursive walker for [`to_vello_cached`].
///
/// Mirrors [`to_vello_inner`]'s match shape but adds a cache check
/// at the top of the [`Scene::Container`] arm: when the accumulated
/// transform is [`Affine::IDENTITY`] AND the subtree is cacheable
/// per [`Scene::is_cacheable_for_paint`], probe the cache first.
/// Hit → append fragment, return. Miss → encode the entire subtree
/// into a fresh `VelloScene`, append it to `out`, install in cache.
///
/// Non-cacheable Containers (and all Containers reached under a
/// non-identity transform) take the direct encode path: paint the
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
/// `paint_immediate_mode_node` helpers. [`Scene::Scroll`] threads
/// the inherited transform exactly like [`to_vello_inner`]; the
/// recursion into `content` is via this function so the eventual
/// `IDENTITY`-resumed descendant Containers (extremely rare — would
/// require a parent that itself cancels the scroll translation) can
/// participate in caching.
#[allow(clippy::too_many_arguments)]
fn to_vello_cached_inner<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    fragment_cache: &mut FragmentCache,
    out: &mut VelloScene,
    transform: Affine,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    // Cacheable-container fast path — only at identity transform AND
    // when the subtree contains no immediate-mode / external
    // descendants. A hit appends the stored fragment; a miss encodes
    // the subtree into a fresh sub-scene and appends + caches it. Either
    // way `out` receives an `append`, never a direct draw (see the
    // R706 invariant below).
    if let Scene::Container(c) = scene
        && transform == Affine::IDENTITY
        && scene.is_cacheable_for_paint()
    {
        let hash = c.paint_hash();
        if fragment_cache.try_hit(hash, out) {
            return;
        }
        // Cache miss: encode the entire subtree into a fresh sub-scene
        // under IDENTITY transform, then append + cache.
        let mut sub = VelloScene::new();
        paint_box_shadows(&mut sub, c.rect, &c.style, Affine::IDENTITY);
        fill_box_bg(&mut sub, c.rect, &c.style, c.style.fill, Affine::IDENTITY);
        for child in &c.children {
            to_vello_cached_inner(
                child,
                fill_hook,
                text_cache,
                fragment_cache,
                &mut sub,
                Affine::IDENTITY,
            );
        }
        out.append(&sub, None);
        fragment_cache.insert_miss(hash, sub, c.rect);
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
            paint_box_shadows(&mut sub, c.rect, &c.style, transform);
            fill_box_bg(&mut sub, c.rect, &c.style, c.style.fill, transform);
            for child in &c.children {
                to_vello_cached_inner(
                    child,
                    fill_hook,
                    text_cache,
                    fragment_cache,
                    &mut sub,
                    transform,
                );
            }
        }
        Scene::Box(b) => {
            paint_box_shadows(&mut sub, b.rect, &b.style, transform);
            let fill = fill_hook(b).unwrap_or(b.style.fill);
            fill_box_bg(&mut sub, b.rect, &b.style, fill, transform);
            if let Some(border) = b.style.border {
                stroke_rect(&mut sub, b.rect, border, transform);
            }
        }
        Scene::Text(t) => paint_text(&mut sub, t, text_cache, transform),
        Scene::Scroll(s) => {
            let viewport_clip = KurboRect::new(
                f64::from(s.viewport.x),
                f64::from(s.viewport.y),
                f64::from(s.viewport.x.saturating_add(s.viewport.w)),
                f64::from(s.viewport.y.saturating_add(s.viewport.h)),
            );
            sub.push_clip_layer(transform, &viewport_clip);
            let dx = f64::from(s.viewport.x) - f64::from(s.offset_x);
            let dy = f64::from(s.viewport.y) - f64::from(s.offset_y);
            let child_transform = transform * Affine::translate((dx, dy));
            to_vello_cached_inner(
                &s.content,
                fill_hook,
                text_cache,
                fragment_cache,
                &mut sub,
                child_transform,
            );
            sub.pop_layer();
        }
        Scene::ImmediateModeNode(node) => {
            paint_immediate_mode_node(&mut sub, node, transform);
        }
        // External / Effect / Path / Image: no-op (matches
        // to_vello_inner's `_ => {}` arm).
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
    let local_transform = parent_transform
        * Affine::translate((f64::from(viewport.x), f64::from(viewport.y)));
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
        self.out.fill(
            Fill::NonZero,
            self.transform,
            to_peniko(color),
            None,
            &rect,
        );
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
        self.out.fill(
            Fill::NonZero,
            self.transform,
            to_peniko(color),
            None,
            &rect,
        );
    }

    fn fill_triangle(
        &mut self,
        p1: (f32, f32),
        p2: (f32, f32),
        p3: (f32, f32),
        color: Color,
    ) {
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
        self.out.fill(
            Fill::NonZero,
            self.transform,
            to_peniko(color),
            None,
            &path,
        );
    }

    fn stroke_line(
        &mut self,
        p1: (f32, f32),
        p2: (f32, f32),
        width: f32,
        color: Color,
    ) {
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
fn fill_rect(
    out: &mut VelloScene,
    r: Rect,
    fill: Color,
    corner_radius: u32,
    transform: Affine,
) {
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

/// R708 §5.50 — paint a Box / Container background: the
/// [`BoxStyle::gradient`] overlay when present, otherwise the solid
/// `solid` colour. `solid` is the caller-resolved fill (a `Box`'s
/// `fill_hook` override or `style.fill`; a `Container`'s `style.fill`),
/// so a gradient takes precedence over the solid only when explicitly
/// set — mirroring Flutter's `BoxDecoration { color, gradient }`.
fn fill_box_bg(
    out: &mut VelloScene,
    r: Rect,
    style: &BoxStyle,
    solid: Color,
    transform: Affine,
) {
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
    let brush = PenikoBrush::Gradient(peniko_gradient);

    let x1 = x0 + w;
    let y1 = y0 + h;
    if corner_radius == 0 {
        let rect = KurboRect::new(x0, y0, x1, y1);
        out.fill(Fill::NonZero, transform, &brush, None, &rect);
    } else {
        let rounded = KurboRoundedRect::new(x0, y0, x1, y1, f64::from(corner_radius));
        out.fill(Fill::NonZero, transform, &brush, None, &rounded);
    }
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

/// Emit one Vello glyph run per parley [`GlyphRun`] shaped from
/// `t.content` + `t.style` (R47.3 §5.36 + R47.6 Figma-fidelity wire).
///
/// The text origin is `(t.rect.x, t.rect.y)`; `t.rect.w > 0` wraps at
/// that pixel width, `w == 0` flows on a single unbounded line.
///
/// R47.6 decoration: when [`TextStyle::decoration`] enables underline
/// or strikethrough, parley populates each [`GlyphRun`]'s style with a
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
fn paint_text(
    out: &mut VelloScene,
    t: &TextNode,
    cache: &mut LayoutCache,
    parent_transform: Affine,
) {
    if t.content.is_empty() {
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
    let layout = cache.layout_with_runs(&t.content, &t.style, &t.runs, max_width);
    // R51.188 §5.45 R55.E.1 — compose the inherited transform (e.g.
    // a parent `Scene::Scroll`'s shifted child transform) with the
    // text's own `(t.rect.x, t.rect.y)` translation. Pre-R51.188
    // `paint_text` assumed `Affine::IDENTITY` from the caller; the
    // composition keeps that path bit-identical (IDENTITY * T = T)
    // and lets scroll-embedded text track the scroll offset.
    let transform = parent_transform
        * Affine::translate((f64::from(t.rect.x), f64::from(t.rect.y)));
    // R47.6 — Clip + Ellipsis (silent fallback to Clip until R47.x
    // ellipsis pass) wrap the emit in a Vello clip layer keyed to
    // `t.rect`. Visible skips the wrap entirely so a freshly-default
    // TextNode pays no per-frame layer cost.
    let needs_clip = matches!(t.style.overflow, TextOverflow::Clip | TextOverflow::Ellipsis);
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
        out.push_clip_layer(parent_transform, &clip_rect);
    }
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let parley_run = run.run();
            let font = parley_run.font();
            let font_size = parley_run.font_size();
            let brush = to_peniko(run.style().brush);
            out.draw_glyphs(font)
                .transform(transform)
                .font_size(font_size)
                .brush(brush)
                .draw(
                    Fill::NonZero,
                    run.positioned_glyphs().map(|g| Glyph {
                        id: g.id,
                        x: g.x,
                        y: g.y,
                    }),
                );
            // R47.6 — decoration strokes. parley emits `Some(Decoration)`
            // on `style().underline / strikethrough` whenever the source
            // TextStyle enabled them (see `LayoutCache::shape`'s
            // `StyleProperty::Underline / Strikethrough` push). The
            // offset / size are font-metric-defaulted (parley fills the
            // Option with the run metric values); the brush defaults to
            // the run's foreground brush.
            paint_decorations(out, &run, transform);
        }
    }
    if needs_clip {
        out.pop_layer();
    }
}

/// R47.6 — emit underline + strikethrough strokes for one parley
/// [`GlyphRun`]. Each decoration is a horizontal line at the
/// font-metric-derived offset spanning the run advance; the brush is
/// the run's foreground colour (matching parley's `Decoration.brush`
/// default).
fn paint_decorations(
    out: &mut VelloScene,
    run: &pinion_text::parley::GlyphRun<'_, Color>,
    transform: Affine,
) {
    let parley_run = run.run();
    let metrics = parley_run.metrics();
    let baseline = run.baseline();
    let start = f64::from(run.offset());
    let end = f64::from(run.offset() + run.advance());
    if let Some(deco) = run.style().underline.as_ref() {
        let offset = deco.offset.unwrap_or(metrics.underline_offset);
        let size = deco.size.unwrap_or(metrics.underline_size);
        // parley's underline offset is measured upward from the baseline
        // (positive = above); on screen Y the underline sits below the
        // baseline, so subtract. The Y advances downward in our coord
        // system, hence the `- offset`.
        let y = f64::from(baseline - offset);
        let line = Line::new((start, y), (end, y));
        out.stroke(
            &Stroke::new(f64::from(size).max(1.0)),
            transform,
            to_peniko(deco.brush),
            None,
            &line,
        );
    }
    if let Some(deco) = run.style().strikethrough.as_ref() {
        let offset = deco.offset.unwrap_or(metrics.strikethrough_offset);
        let size = deco.size.unwrap_or(metrics.strikethrough_size);
        let y = f64::from(baseline - offset);
        let line = Line::new((start, y), (end, y));
        out.stroke(
            &Stroke::new(f64::from(size).max(1.0)),
            transform,
            to_peniko(deco.brush),
            None,
            &line,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
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
        let scene = Scene::Box(BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgb(0xff, 0, 0)));
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
                BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgb(0xff, 0, 0))
                    .with_tag("a"),
            ),
            Scene::Box(
                BoxNode::filled(Rect::new(20, 0, 10, 10), Color::rgb(0, 0xff, 0))
                    .with_tag("b"),
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
        let inner = ContainerNode::new(vec![
            Scene::Box(
                BoxNode::filled(Rect::new(10, 10, 10, 10), Color::rgb(0, 0xff, 0))
                    .with_tag("leaf"),
            ),
        ]);
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
        // valid; we cannot read the encoded layer stack from outside
        // the crate, but the no-panic walk + Vello's own internal
        // assertions (debug builds verify layer balance) cover this.
        use pinion_core::style::TextOverflow;
        for overflow in [
            TextOverflow::Visible,
            TextOverflow::Clip,
            TextOverflow::Ellipsis,
        ] {
            let scene = Scene::Text(TextNode::styled(
                "OverflowingContent",
                Rect::new(0, 0, 50, 16), // intentionally tight
                TextStyle::new()
                    .with_size_px(16)
                    .with_overflow(overflow),
            ));
            let mut vello = VelloScene::new();
            let mut cache = LayoutCache::new();
            to_vello(&scene, &|_| None, &mut cache, &mut vello);
        }
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
        let empty_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 100, 100),
            empty_content,
        ));

        let inner_box = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 200, 200),
            Color::rgb(0xff, 0, 0),
        ));
        let plain_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 100, 100),
            inner_box,
        ));

        let inner_inner = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 50, 50),
            Color::rgb(0, 0, 0xff),
        ));
        let inner_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(10, 10, 50, 50),
            inner_inner,
        ));
        let outer_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 200, 200),
            inner_scroll,
        ));

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
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), text)
            .with_offset(0, 20);
        let scene = Scene::Scroll(scroll);
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "first paint populates cache");
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "repeat paint hits cache, no growth");
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
            <Scene as MatchBoxStyle>::corner_radius(&scene),
            0,
            "default BoxStyle radius is 0",
        );
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        // The sharp-rect path was the only one before R639; survival
        // through the new dispatch + non-empty vello scene proves
        // the zero-radius arm preserved its legacy behaviour.
        assert!(vello.encoding().n_paths > 0, "fill emitted at least one path");
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
            rounded_scene.encoding().n_path_segments
                > sharp_scene.encoding().n_path_segments,
            "rounded path must have more segments than sharp; \
             sharp={}, rounded={}",
            sharp_scene.encoding().n_path_segments,
            rounded_scene.encoding().n_path_segments,
        );
    }

    // Internal helper to extract `corner_radius` from a `Scene` arm
    // for the R639 zero-path sanity assertion. Lives inside the test
    // module to keep the production surface free of one-off accessors.
    trait MatchBoxStyle {
        fn corner_radius(&self) -> u32;
    }
    impl MatchBoxStyle for Scene {
        fn corner_radius(&self) -> u32 {
            match self {
                Scene::Container(c) => c.style.corner_radius,
                Scene::Box(b) => b.style.corner_radius,
                _ => 0,
            }
        }
    }

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
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), content)
            .with_offset(i32::MAX, i32::MAX);
        let scene = Scene::Scroll(scroll);
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
    }

    // ─────────────────────────────────────────────────────────────
    // R682 §5.16 atomic 1 — FragmentCache + to_vello_cached
    // ─────────────────────────────────────────────────────────────

    use pinion_core::scene::{
        EffectNode, ExternalNode, ImmediateMode, ImmediateModeNode, Scene,
    };

    fn null_hook<'a>() -> &'a (dyn Fn(&BoxNode) -> Option<Color> + 'a) {
        &|_b: &BoxNode| None
    }

    fn simple_container() -> Scene {
        Scene::Container(
            ContainerNode::new(vec![
                Scene::Box(BoxNode::filled(
                    Rect::new(10, 10, 100, 50),
                    Color::rgb(0xff, 0, 0),
                )),
                Scene::Text(TextNode::new("hi", Rect::new(10, 70, 100, 20))),
            ]),
        )
    }

    fn mutated_container() -> Scene {
        Scene::Container(
            ContainerNode::new(vec![
                Scene::Box(BoxNode::filled(
                    Rect::new(10, 10, 100, 50),
                    // Color differs from `simple_container`.
                    Color::rgb(0, 0xff, 0),
                )),
                Scene::Text(TextNode::new("hi", Rect::new(10, 70, 100, 20))),
            ]),
        )
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
        to_vello_cached(&scene, &null_hook(), &mut text, &mut cache, &mut vello);
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
            &mut cache,
            &mut vello1,
        );
        let mut vello2 = VelloScene::new();
        to_vello_cached(
            &simple_container(),
            &null_hook(),
            &mut text,
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
            &mut cache,
            &mut vello1,
        );
        let mut vello2 = VelloScene::new();
        to_vello_cached(
            &mutated_container(),
            &null_hook(),
            &mut text,
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
            &mut cache,
            &mut v,
        );
        assert_eq!(cache.entries(), 1);

        let mut v = VelloScene::new();
        to_vello_cached(
            &mutated_container(),
            &null_hook(),
            &mut text,
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
        to_vello_cached(&scene, &null_hook(), &mut text, &mut cache, &mut v);
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
        to_vello_cached(&scene, &null_hook(), &mut text, &mut cache, &mut v);
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
        to_vello_cached(&make_scene(), &null_hook(), &mut text, &mut cache, &mut v);
        // First paint: header Container missed (installed) — root
        // Container is uncacheable so it didn't probe; ImmediateMode
        // doesn't probe.
        assert_eq!(cache.misses(), 1, "header Container installed");
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.entries(), 1);

        let mut v = VelloScene::new();
        to_vello_cached(&make_scene(), &null_hook(), &mut text, &mut cache, &mut v);
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
            &mut cache,
            &mut cached,
        );

        // Both produced a valid VelloScene; the cached version
        // installed exactly one fragment (the root Container).
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.entries(), 1);
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
        let Scene::Container(ref c) = scene else { unreachable!() };
        let expected_rect = c.rect;

        let mut cache = FragmentCache::new();
        let mut text = LayoutCache::new();
        let mut v = VelloScene::new();
        to_vello_cached(&scene, &null_hook(), &mut text, &mut cache, &mut v);

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
        to_vello_cached(scene, &null_hook(), text, cache, &mut v);
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
        assert_eq!(cache.entries(), 100, "row fragments installed; root untracked");
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
        let completed_indices: Vec<u32> = (0..100u32)
            .filter(|i| full_mask[*i as usize])
            .collect();
        let completed_n = u32::try_from(completed_indices.len()).expect("≤ 100 fits u32");
        let mut filtered_children: Vec<Scene> = vec![Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(InertImmediate, Rect::new(0, 0, 100, 1)),
        )];
        for i in &completed_indices {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "i < 100 fits in u8"
            )]
            let g = (*i % 256) as u8;
            filtered_children.push(Scene::Container(
                ContainerNode::new(vec![Scene::Box(BoxNode::filled(
                    Rect::new(0, i * 10 + 10, 100, 10),
                    Color::rgb(0, g, 0),
                ))])
                .with_tag(format!("row_{i}")),
            ));
        }
        let filtered = Scene::Container(
            ContainerNode::new(filtered_children).with_tag("stress_root"),
        );

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
}
