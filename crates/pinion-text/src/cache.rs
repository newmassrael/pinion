//! R47.2 §5.36 — [`LayoutCache`]: LRU-bounded cache from
//! `(text, style, max_width)` to a shaped [`Layout`].
//!
//! Realizes the §5.36 R47.2 `LayoutCache` output: text content that does
//! not change frame-to-frame (button labels, static UI strings) shapes
//! once and reuses on subsequent frames. Per-frame parley work is
//! reduced to a hashmap probe in the steady state.
//!
//! `FontContext` and `LayoutContext` live inside the cache so callers
//! never own parley state directly. The cache is intentionally not
//! `Send` / `Sync` — parley's contexts hold single-thread state that
//! aligns with §6.3 view-fn purity (sync, single-thread). Per-thread
//! caches are the textbook pattern; multi-thread shaping is R47.x
//! carry.
//!
//! R1447 §5.36 — the `FontContext` is built on the first *shape*, not on
//! construction. See [`LayoutCache`] for why that distinction is load-
//! bearing rather than a micro-optimisation.
//!
//! R1521 §5.36 — the capacity **adapts to the working set** rather than
//! standing at a fixed number. See [`LayoutCache`]'s "the capacity is
//! earned" section: a fixed LRU bound smaller than a cyclic working set
//! has a 0% hit rate, which is the one failure mode this cache can have.

use crate::glyph_run::{self, PositionedRun};
use crate::layout::Layout;
use lru::LruCache;
use parley::fontique::Blob;
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, GenericFamily,
    LayoutContext, LineHeight as ParleyLineHeight, StyleProperty,
};
use pinion_core::reactive::SystemFontStatus;
use pinion_core::scene::StyleRun;
use pinion_core::style::{
    Color, FontFamily as PinFontFamily, FontStyle, GenericFontFamily, LineHeight, TextAlign,
    TextStyle,
};
use pinion_core::text_cache_stats::TextCacheStats;
use std::borrow::Cow;
use std::num::NonZeroUsize;

/// Cache key. Captures the input the cache uses to identify a parley
/// `Layout` output: text content, style, and optional max width (the
/// line break point in pixels). `max_width = None` means no wrap
/// (single line / unbounded).
///
/// # Conservative key (R587 §5.36)
///
/// The key includes `TextStyle` *in full*, which means `fg_color`
/// participates via `Hash + Eq`. Strictly, `fg_color` lands in parley as
/// `StyleProperty::Brush`, which is paint metadata: it carries through
/// to glyph runs but does not influence shape (glyph cluster
/// composition, advances, line breaks). So the current key is
/// *over-specified* — two layouts that differ only in `fg_color` are
/// shape-identical, yet the cache treats them as distinct entries.
///
/// Effect during the R57.X.theme-fade cross-fade: the active palette's
/// `fg_color` is in-flight (linear-space spring lerp toward the target
/// for ~200ms / ~12 frames @ 60fps), so each frame's view-fn emits a
/// `TextStyle` with a fresh `fg_color` and the cache treats every frame
/// as a miss — parley re-shapes per frame for the duration of the
/// fade.
///
/// Round 587 measurement (`hello-textfield` single-line label) places
/// the per-frame shape cost at ~1-2 ms, and a 12-frame fade therefore
/// burns ~6-12% of the 60fps frame budget — below the visible jitter
/// threshold for this widget. The same arithmetic with a long /
/// multi-line buffer (a multiline editor consumer, R47.x carry) would
/// push per-frame shape past 5-10 ms and into visible regression
/// territory, so the over-specification is a *latent* perf hazard,
/// not a current one.
///
/// # Why not split now
///
/// The textbook substrate fix is to separate *shape-determinants*
/// (`font_family`, `font_size_px`, `font_weight`, `font_style`,
/// `letter_spacing`, `line_height`, `text_align`) from
/// *paint-metadata* (`fg_color`, decoration colors), key only on the
/// former, and apply the latter as a post-cache brush override on the
/// returned `Layout`. That mirrors the parley boundary
/// (`StyleProperty::Brush` is the canonical paint-only property) and
/// trades the current 1-call API for either a `TextStyle` type-split
/// or a `layout_with_brush(...)` overload. It also widens the public
/// surface — both `pinion-core::style::TextStyle` and every consumer
/// that shapes via the cache.
///
/// Rule of Three discipline (cf. `abstraction-needs-second-consumer`):
/// the only consumer that exercises in-flight `fg_color` today is
/// the R57.X.theme-fade animated palette, and the measured impact is
/// well below the visible threshold for `hello-textfield`. Defer the
/// split until a second consumer (a multi-line text input / paragraph
/// editor) crosses the visible threshold; carry tracked in
/// `[[r57-x-theme-fade-substrate]]` Rule of Three list. See
/// `different_fg_color_creates_new_entry` for the regression that
/// pins the current behavior so the split lands deliberately.
/// # R713 styled runs
///
/// `runs` carries the [`StyleRun`] spans (rich / multi-style text). It
/// participates in the key via `Hash + Eq`: a node whose runs differ
/// (a highlight toggling a span's weight) is a distinct cache entry, so
/// the layout re-shapes when the styling changes. The common
/// single-style node has `runs.is_empty()` and the vector contributes
/// nothing to the hash beyond its zero length.
#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    style: TextStyle,
    runs: Vec<StyleRun>,
    max_width: Option<u32>,
}

/// R1531 §5.36 — one cache entry: the shaped layout, plus the draw list
/// derived from it once a painter asks for one.
///
/// `runs` is `None` until [`LayoutCache::positioned_runs`] is first called for
/// this key, and that laziness is load-bearing rather than an optimisation
/// reflex: this cache has callers that shape in order to **measure**
/// ([`crate::LayoutCacheTextMetrics`], the caret geometry in
/// [`crate::caret_rect_for_byte_offset`], `pinion_tui`'s cell-grid measure arm)
/// and never paint what they shaped. Deriving eagerly on every miss would bill
/// them for a list nobody replays.
struct CachedLayout {
    layout: Layout,
    runs: Option<Vec<PositionedRun>>,
}

/// LRU-bounded cache over [`Layout`] values keyed by
/// `(text, style, max_width)`. Construct via [`LayoutCache::new`]
/// (default capacity) or [`LayoutCache::with_capacity`] for explicit
/// sizing.
///
/// # R1447 §5.36 — constructing a cache reads no fonts
///
/// `font_cx` is built on the first call that actually shapes, not in the
/// constructor. `FontContext::new()` enumerates **every system font**
/// through the platform font API (fontconfig on Linux): measured at
/// **25.5 ms on a 635-font box**, and fontique caches nothing across
/// instances — `CollectionOptions::system_fonts` is `true` by default and
/// each `Collection::new` runs a fresh `FcInitLoadConfig` scan. That last
/// clause is the load-bearing one (a process-cached scan would leave nothing
/// worth deferring) and R1448.1 observes it rather than reading it off
/// fontique's source: `system_scan_reruns_for_each_context` in
/// `tests/font_less_host.rs` adds a face to the configured directory between
/// two contexts and shows only the second one sees it.
///
/// That cost is not the interesting part. The load-bearing consequence is
/// that a `LayoutCache` used by a **caller that never shapes** paid for a
/// resource it never touched — and the §2 #6 GUI/TUI dual has exactly such
/// a caller. `pinion_tui`'s measure arm lays every `Scene::Text` leaf out
/// on the cell grid and never defers to parley (the terminal has no
/// fonts), so on the TUI path the only consumer of `font_cx`
/// (`shape`'s `ranged_builder`) is unreachable. Eagerly it still
/// scanned; lazily it does not exist.
///
/// The consequence that is a *correctness* one, not a cost one: on a host
/// with no matching fonts at all — a slim container, a CI image with no
/// font package — `FontContext::new()` **panics** inside fontique
/// (`backend/fontconfig.rs:685`, `config.font_sort(..).unwrap()` on
/// `Err(NoMatch)`, reached while it populates the generic-family map). So
/// eager construction made a font-less host fatal to the font-less
/// backend: 51 of `pinion-tui`'s 136 unit tests died there. Deferring the
/// build is the same root fix as the cost — both come from building what
/// the caller will not use.
///
/// The laziness is invisible to the public surface: every shaping entry
/// point builds the context on demand, so a caller that *does* shape pays
/// the identical cost at the identical count, one construction later.
/// [`Self::font_scans`] reports how many times it has been built, which is
/// what pins the TUI guarantee as an assertion rather than a claim.
///
/// # R1521 §5.36 — the capacity is earned, not guessed
///
/// A fixed-capacity LRU has exactly one failure mode, and a UI frame walks
/// straight into it. Painting is a **cyclic** access pattern: every frame
/// visits the same labels in the same order. LRU is pathological on a cycle
/// longer than its capacity — each entry is evicted by the one requested
/// just before it comes round again — so the hit rate is not *degraded* but
/// **0%**, and it is 0% on every subsequent frame, forever.
///
/// Measured on this box (release, `Scene::Text` leaves through the paint
/// walk, one shell-sized cache), the old fixed 256 produced a cliff:
///
/// | text leaves | steady-state paint | shapes per frame | after R1521 | settles at |
/// |------------:|-------------------:|-----------------:|------------:|-----------:|
/// | 256         | 0.53 ms            | 0                | 0.40 ms     | 256        |
/// | 300         | 5.35 ms            | 300              | **0.43 ms** | 512        |
/// | 512         | 11.9 ms            | 512              | **0.76 ms** | 512        |
/// | 1200        | 27.4 ms            | 1200             | **1.59 ms** | 2048       |
///
/// A **17% increase in content multiplied the per-frame cost by ten**, and
/// 1200 leaves — a 30-column data grid with 40 visible rows, an ordinary
/// pro-tool scene — cost 1.6x the entire 60fps budget on shaping alone.
/// That is the shape of a cliff, not of a cost curve, which is what makes
/// it worth removing rather than tuning.
///
/// Raising the constant moves the cliff instead of removing it: whatever
/// number is chosen, the scene one leaf larger falls off it just as hard.
/// So the capacity **grows on proof that it was too small**. The proof is a
/// miss on a key this cache itself evicted — the ghost list of 2Q / ARC
/// (Megiddo & Modha, 2003). A key that comes back after being evicted is a
/// witness that the working set did not fit; a key that never comes back is
/// a scan, and a scan must not grow anything.
///
/// That distinction is the whole design, and it is what
/// [`Self::growths`] makes checkable:
///
/// - **cyclic** (row labels revisited every frame) — evicted keys return,
///   so the cache doubles until the set fits, then stops.
/// - **scan** (a streaming log whose lines are never revisited) — evicted
///   keys never return, so the capacity stays where it started no matter
///   how many million lines pass through.
///
/// Growth is bounded by [`Self::MAX_CAPACITY`]. Entries are ~3.1 KB
/// measured (a 24-character label, RSS delta over 20,000 entries), so the
/// bound is a memory statement: ~26 MB if a scene ever proves it needs the
/// whole thing, against ~0.8 MB for the 256 that used to be the ceiling.
/// A caller that needs a hard bound states one with
/// [`Self::with_max_capacity`]; that is the only way to get the old
/// fixed-ceiling behaviour back, and it is now a decision rather than a
/// default.
pub struct LayoutCache {
    inner: LruCache<LayoutKey, CachedLayout>,
    /// R1521 — hashes of keys this cache evicted. A miss whose key hashes
    /// into here is the witness that [`Self::inner`] is too small for the
    /// caller's working set; see the type doc.
    ///
    /// **Sized to [`Self::max_capacity`], not to `inner`.** A ghost list as
    /// long as the cache detects only working sets up to twice the capacity:
    /// beyond that, a cycle's own evictions push the earlier evidence out
    /// before the cycle comes round to supply it. Measured against the case
    /// that motivated R1521 — capacity 256, working set 1,200 — a
    /// capacity-sized ghost list never fires at all. The list has to span the
    /// largest working set the cache is *allowed* to serve, which is the
    /// ceiling.
    ///
    /// That sizing carries a property worth naming: a working set larger than
    /// [`Self::max_capacity`] produces no ghost hits either, so the cache does
    /// not grow for it. That is correct rather than a gap — such a set cannot
    /// be held at any permitted capacity, so growing toward the ceiling would
    /// buy nothing and cost the memory.
    ///
    /// Hashes rather than keys: the question asked of this list is only
    /// "recently evicted?", and a `u64` entry costs no heap while a
    /// [`LayoutKey`] carries an owned `String` and a `Vec`. A collision would
    /// authorise one growth that no eviction earned — bounded, self-limiting,
    /// and at 8,192 live entries about 2e-12 likely.
    ///
    /// The map is allocated at full size on construction, which is what a
    /// ceiling-sized list costs whether or not the cache ever grows: **33 kB
    /// per `LayoutCache`**, measured as the RSS delta over 100 fresh caches.
    /// Every caller that builds one per call rather than per shell is a
    /// documented one-shot (`pinion_tui::render_one_frame`, the drag-chip
    /// repaint), so this lands on no hot path; a future per-frame constructor
    /// would want [`LayoutCache::with_max_capacity`] with a small ceiling
    /// rather than the default.
    ghosts: LruCache<u64, ()>,
    /// R1521 — the ceiling [`Self::grow`] will not pass.
    max_capacity: NonZeroUsize,
    /// R1521 — how many times the capacity doubled. Reported by
    /// [`Self::growths`].
    growths: u32,
    /// `None` until the first shape. See the type doc — this is the
    /// system-font scan, deferred.
    font_cx: Option<FontContext>,
    /// How many times [`Self::shape`] has built `font_cx`. The invariant is
    /// "at most once", and a count is what states that; see
    /// [`Self::font_scans`].
    font_scans: u32,
    /// R1454 §5.36 — how many times [`Self::shape`] actually ran the shaper,
    /// i.e. how many cache MISSES this instance has served. Reported by
    /// [`Self::shapes`].
    ///
    /// A counter rather than a flag for the reason R1447 replaced its own
    /// bool: the invariant a caller cares about is not "did it shape" but
    /// "how many times *this frame*", and only a number can answer that. A
    /// working set larger than the cache's capacity re-shapes on every pass,
    /// and nothing else in the surface makes that visible.
    shapes: u64,
    /// R1531 §5.36 — how many times a shaped layout's draw list has been
    /// derived. Reported by [`Self::run_builds`].
    ///
    /// The counter that states the whole of R1531 as an assertion: the walk
    /// over parley's runs is a pure function of the layout, so a scene that
    /// paints the same text on 60 consecutive frames must move this by the
    /// number of *distinct* layouts, not by 60 times that. Nothing else
    /// distinguishes "the draw list was replayed" from "the draw list was
    /// rebuilt and looked the same" — both paint identical pixels, and one of
    /// them costs 2.9x (see [`crate::glyph_run`]).
    run_builds: u64,
    /// R1448 — set when `font_cx` is built; `NotProbed` until then.
    font_status: SystemFontStatus,
    /// R1448 — families made selectable by [`Self::register_font_data`], in
    /// registration order, deduplicated.
    app_families: Vec<String>,
    /// R1472 §5.36 — the family an unset [`TextStyle::font_family`] resolves
    /// to; `None` keeps parley's platform stack. See
    /// [`Self::set_default_font_family`].
    default_family: Option<PinFontFamily>,
    layout_cx: LayoutContext<Color>,
}

/// R1531 §5.36 — derive `key`'s draw list if it has none, counting the
/// derivation, and return it.
///
/// A free function over the two fields rather than a `&mut self` method, the
/// same shape (and for the same reason) as [`ensure_font_context`] below: the
/// counter and the entry are disjoint parts of the cache, and a method taking
/// `&mut self` would borrow both through one reference, so incrementing the
/// counter would conflict with the borrow that returns the list.
///
/// `key` must already be present — [`LayoutCache::ensure_entry`] is the
/// caller's precondition.
fn ensure_runs<'a>(
    inner: &'a mut LruCache<LayoutKey, CachedLayout>,
    run_builds: &mut u64,
    key: &LayoutKey,
) -> &'a [PositionedRun] {
    let entry = inner
        .get_mut(key)
        .expect("entry just inserted on cache miss");
    if entry.runs.is_none() {
        *run_builds += 1;
        entry.runs = Some(glyph_run::positioned_runs(
            &entry.layout,
            &key.style,
            &key.runs,
        ));
    }
    entry.runs.as_deref().expect("derived above if absent")
}

/// R1448 §5.36 — build `slot`'s [`FontContext`] if absent, recording the probe
/// outcome and counting the scan.
///
/// A free function over the three fields rather than a `&mut self` method: the
/// shaping path needs `layout_cx` borrowed at the same time, and a method
/// taking `&mut self` would borrow the whole cache. Taking the slots
/// explicitly keeps the borrows disjoint while both callers
/// ([`LayoutCache::shape`] and [`LayoutCache::register_font_data`]) share one
/// copy of the initialisation.
fn ensure_font_context<'a>(
    slot: &'a mut Option<FontContext>,
    status: &mut SystemFontStatus,
    scans: &mut u32,
) -> &'a mut FontContext {
    if slot.is_none() {
        let (cx, probed) = crate::font_source::build_font_context();
        *status = probed;
        *scans += 1;
        *slot = Some(cx);
    }
    slot.as_mut().expect("built directly above")
}

impl LayoutCache {
    /// Starting cache capacity (cached layouts). A `NonZeroUsize`
    /// compile-time constant so [`LayoutCache::new`] needs no runtime
    /// unwrap.
    ///
    /// R1521 §5.36 — a *starting* capacity since the ceiling became
    /// [`Self::MAX_CAPACITY`]: a cache that proves this is too small for its
    /// caller grows past it. See the type doc.
    pub const DEFAULT_CAPACITY: NonZeroUsize = NonZeroUsize::new(256).expect("256 is non-zero");

    /// R1521 §5.36 — the ceiling an adaptively-grown cache will not pass.
    ///
    /// 8,192 entries at the measured ~3.1 KB each is ~26 MB, and it takes a
    /// scene of 8,192 simultaneously-painted text leaves to reach it — a
    /// dense 4K pro-tool layout (180 rows x 30 columns) is about 5,400. The
    /// bound exists so the growth rule cannot be turned into an unbounded
    /// allocation by an adversarial access pattern, not because any measured
    /// scene approaches it.
    pub const MAX_CAPACITY: NonZeroUsize = NonZeroUsize::new(8192).expect("8192 is non-zero");

    /// Construct a cache that starts at `capacity` slots and may grow to
    /// [`Self::MAX_CAPACITY`]. Use [`LayoutCache::new`] for the default
    /// start, or [`LayoutCache::with_max_capacity`] to state a hard bound.
    #[must_use]
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        Self::with_max_capacity(capacity, Self::MAX_CAPACITY.max(capacity))
    }

    /// R1521 §5.36 — construct a cache that starts at `capacity` and grows no
    /// further than `max_capacity`.
    ///
    /// Passing `max_capacity == capacity` pins the capacity: the cache never
    /// grows, and a working set larger than it re-shapes every pass exactly as
    /// a fixed-capacity LRU always did. That behaviour is still reachable
    /// because a caller may genuinely need a hard memory bound more than it
    /// needs hits — a preview thumbnail pass, a one-shot PDF render. It is no
    /// longer the *default*, which is the whole of R1521: the cliff is now
    /// something a caller opts into with a number it chose, not something
    /// every caller inherits from a constant.
    ///
    /// `max_capacity` is clamped up to `capacity` — a ceiling below the floor
    /// would describe no cache at all.
    #[must_use]
    pub fn with_max_capacity(capacity: NonZeroUsize, max_capacity: NonZeroUsize) -> Self {
        let max_capacity = max_capacity.max(capacity);
        Self {
            inner: LruCache::new(capacity),
            ghosts: LruCache::new(max_capacity),
            max_capacity,
            growths: 0,
            font_cx: None,
            font_scans: 0,
            shapes: 0,
            run_builds: 0,
            font_status: SystemFontStatus::NotProbed,
            app_families: Vec::new(),
            default_family: None,
            layout_cx: LayoutContext::new(),
        }
    }

    /// Construct a cache starting at [`Self::DEFAULT_CAPACITY`] slots.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// Shape `text` with `style` and (optionally) wrap at `max_width`
    /// pixels. Returns a reference to the cached Layout and promotes
    /// it to most-recently-used. Subsequent calls with the same inputs
    /// return the same entry without re-running parley.
    ///
    /// # Panics
    ///
    /// Never panics in practice. The internal `expect()` upholds the
    /// `LruCache` invariant that a key just inserted via `put` is
    /// retrievable via `get` on the same call sequence; an LRU
    /// implementation violating that would be a backing-library bug.
    pub fn layout(&mut self, text: &str, style: &TextStyle, max_width: Option<u32>) -> &Layout {
        self.layout_with_runs(text, style, &[], max_width)
    }

    /// R713 §5.36 — shape `text` with a base `style` plus styled-run
    /// overrides and (optionally) wrap at `max_width` pixels.
    ///
    /// `runs` is the [`StyleRun`] list: the base `style` is pushed as
    /// the default, then each run's fully-resolved style is pushed over
    /// its UTF-8 byte range (later runs win on overlap, matching
    /// parley's range resolution). `runs.is_empty()` is exactly
    /// [`Self::layout`] — the single-style fast path — and produces a
    /// byte-identical cache key, so pre-R713 callers are unaffected.
    ///
    /// # Panics
    ///
    /// Never panics in practice — same `LruCache` invariant as
    /// [`Self::layout`].
    pub fn layout_with_runs(
        &mut self,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> &Layout {
        let key = LayoutKey {
            text: text.to_owned(),
            style: style.clone(),
            runs: runs.to_vec(),
            max_width,
        };
        self.ensure_entry(&key, text, style, runs, max_width);
        &self
            .inner
            .get(&key)
            .expect("entry just inserted on cache miss")
            .layout
    }

    /// R1531 §5.36 — the **draw list** for `text`: its shaped glyph runs,
    /// positioned, with every decoration resolved. What a painter replays.
    ///
    /// Same key, same entry and same eviction as [`Self::layout_with_runs`] —
    /// this is the second half of one derivation, not a second cache. Derived
    /// on the first call for an entry and reused by every call after, which is
    /// what [`Self::run_builds`] reports.
    ///
    /// A painter should prefer this to walking [`Self::layout_with_runs`]'s
    /// [`Layout`] itself: the walk it replaces is 2.9x the cost of the encode
    /// it feeds (measured — see [`crate::glyph_run`]), and re-running it per
    /// frame produces a list identical to the one already held.
    ///
    /// # Panics
    ///
    /// Never panics in practice — same `LruCache` invariant as
    /// [`Self::layout`].
    pub fn positioned_runs(
        &mut self,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> &[PositionedRun] {
        let key = LayoutKey {
            text: text.to_owned(),
            style: style.clone(),
            runs: runs.to_vec(),
            max_width,
        };
        self.ensure_entry(&key, text, style, runs, max_width);
        ensure_runs(&mut self.inner, &mut self.run_builds, &key)
    }

    /// R1531 §5.36 — put `key`'s entry in the cache, shaping on a miss.
    ///
    /// The miss path shared by [`Self::layout_with_runs`] and
    /// [`Self::positioned_runs`]. It is one function rather than two because
    /// the growth rule below is stated once: a second copy would be a second
    /// place for the ghost bookkeeping to drift.
    ///
    /// It returns nothing, and that is the point — a caller that got the entry
    /// back would hold a borrow of the whole cache, which is exactly what
    /// [`Self::positioned_runs`] cannot have while it counts a derivation.
    /// Each caller takes the borrow it needs afterwards, at the cost of one
    /// extra hash on the hit path and none on the miss path.
    fn ensure_entry(
        &mut self,
        key: &LayoutKey,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) {
        if !self.inner.contains(key) {
            // R1521 §5.36 — the witness. This key missing is ordinary; this
            // key missing *after this cache evicted it* means the working set
            // did not fit, because the caller came back for something the
            // capacity forced out. Popping it also retires the evidence, so
            // one eviction can only justify one growth.
            if self.ghosts.pop(&ghost_hash(key)).is_some() {
                self.grow();
            }
            let layout = self.shape(text, style, runs, max_width);
            // `push` reports what left. Reached only on a miss, so a returned
            // pair is always an eviction rather than a same-key replacement.
            let entry = CachedLayout { layout, runs: None };
            if let Some((evicted, _)) = self.inner.push(key.clone(), entry) {
                self.ghosts.put(ghost_hash(&evicted), ());
            }
        }
    }

    /// R1521 §5.36 — double the capacity, bounded by [`Self::max_capacity`].
    ///
    /// Doubling rather than growing by the measured shortfall, and the
    /// distinction is worth stating because the shortfall *is* measurable: at
    /// the moment a witness fires, `ghosts.len()` is the number of distinct
    /// keys evicted since the last growth, so `capacity + ghosts.len()` sizes
    /// a purely cyclic working set in one step (measured: 256 + 44 = exactly
    /// the 300-leaf scene). It is rejected because that estimate is only sound
    /// when the ghost population is entirely cyclic. Mix a scan into the frame
    /// — a streaming log beside a stable label set — and the ghosts are mostly
    /// keys nobody will ask for again, so the "shortfall" is inflated by
    /// content that was never cacheable, in one jump, with no second
    /// observation to correct it.
    ///
    /// Doubling is robust to what the ghost population happens to contain: it
    /// never over-shoots by more than 2x, it re-measures after each step, and
    /// its convergence is bounded at `log2(MAX_CAPACITY / start)` = five
    /// frames from the default. Measured, a 1,200-leaf scene settles at 2,048
    /// on the fourth frame and a 3,000-leaf scene at 4,096 on the fifth.
    ///
    /// The ghost list is not resized here — it is already sized to
    /// [`Self::max_capacity`], for the reason its field doc gives. It is
    /// **cleared**, and that is what keeps growth proportionate.
    ///
    /// Every ghost in the list was evicted under the capacity that just
    /// doubled, so the list holds one round of evidence for a question that
    /// has now been answered. Spending it entry by entry makes the cache
    /// react once per *evicted key* instead of once per *undersizing*: a
    /// 300-leaf scene against 256 slots arrives at pass two with 44 ghosts and
    /// grows five times on them, landing at 8,192 for a working set that fits
    /// in 512 (measured — this is what the round's first implementation did).
    /// Clearing retires the whole round, so the next growth needs a key
    /// evicted under the *new* capacity to justify it. Convergence takes a few
    /// frames rather than one, and lands on the capacity the scene actually
    /// needs.
    fn grow(&mut self) {
        let current = self.inner.cap();
        if current >= self.max_capacity {
            return;
        }
        let doubled = current.saturating_mul(NonZeroUsize::new(2).expect("2 is non-zero"));
        let next = doubled.min(self.max_capacity);
        self.inner.resize(next);
        self.ghosts.clear();
        self.growths += 1;
    }

    /// Number of currently cached entries (test + diagnostic surface).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the cache holds zero entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// R1447 §5.36 — how many times this cache has enumerated system fonts.
    ///
    /// `0` until the first call that shapes, `1` forever after (see the type
    /// doc for why the build is deferred). A *count*, not a `has_it` flag,
    /// because the invariant this cache owes its callers is "at most once"
    /// and a boolean cannot express that: a cache that rebuilt its
    /// `FontContext` on every shape — paying the ~25 ms scan per cache miss
    /// instead of per process — reads identically through a flag, and a
    /// counterfactual confirmed that such a regression passes every
    /// assertion a flag can carry.
    ///
    /// This is a diagnostic, not a control: no behavior branches on it, and
    /// a caller cannot make shaping cheaper by consulting it. It exists so
    /// the §2 #6 guarantee "a TUI frame reads no fonts" is an assertion over
    /// the real layout pass rather than a claim about which branch is
    /// reachable — and so a consumer profiling a frame can tell what its
    /// cache actually paid.
    #[must_use]
    pub fn font_scans(&self) -> u32 {
        self.font_scans
    }

    /// R1454 §5.36 — how many times this cache has run the shaper: its cache
    /// MISS count since construction.
    ///
    /// The diagnostic for the one failure mode an LRU-bounded measurement
    /// cache has. A caller measuring a **bounded** working set sees this stop
    /// climbing once the set is warm; a caller whose per-pass working set
    /// exceeds the capacity sees it climb by the full set size on every pass,
    /// because each entry evicts the one the next pass wants.
    ///
    /// Why that matters, measured (release, this machine, short labels):
    /// a shape **miss costs 18.5 us** and a **hit 118 ns** — 157x. So a pass
    /// over 300 strings that thrashes costs **5.6 ms**, a third of a 60fps
    /// frame, *every frame*; at 1000 strings it is over budget on its own.
    ///
    /// R1521 §5.36 — that is why the capacity is no longer fixed. A cache
    /// that grows on proof of undersizing (see the type doc) drives this
    /// counter back to "stops climbing once warm" for a cyclic working set of
    /// any size up to [`Self::MAX_CAPACITY`], so a thrashing `shapes()` now
    /// means one of two specific things: a working set past that ceiling, or
    /// a caller that pinned its own bound via
    /// [`Self::with_max_capacity`]. Reading it with [`Self::growths`] says
    /// which. A cache is still not by itself a strategy for a *measurement*
    /// pass — a pass that measures rows nobody will look at should measure
    /// fewer, which is what Qt's `QHeaderView::resizeContentsPrecision`
    /// bounds — but a *paint* pass has no such freedom: it must visit every
    /// leaf it paints, so the cache is the only place the cost can go.
    ///
    /// Cheap to read (a field), so a consumer can gate a debug assertion on
    /// it or a profiler can sample it per frame.
    #[must_use]
    pub fn shapes(&self) -> u64 {
        self.shapes
    }

    /// R1531 §5.36 — how many times a shaped layout's draw list has been
    /// derived by [`Self::positioned_runs`].
    ///
    /// Bounded above by [`Self::shapes`] plus the entries a caller measured
    /// before painting, and in a steady-state frame it should not move at all:
    /// the list is a pure function of a layout that did not change. A profiler
    /// seeing it climb frame after frame is seeing the same defect
    /// [`Self::shapes`] climbing means, one derivation later — either the
    /// working set does not fit, or the *keys* are churning (the classic case
    /// being a colour crossfade, which is part of the cache key today).
    #[must_use]
    pub fn run_builds(&self) -> u64 {
        self.run_builds
    }

    /// R1521 §5.36 — the cache's current capacity in entries.
    ///
    /// Starts at what the constructor was given and rises toward
    /// [`Self::max_capacity`] as the working set proves it has to. Reading it
    /// alongside [`Self::len`] is how a profiler tells "warm and roomy" from
    /// "full and about to evict".
    #[must_use]
    pub fn capacity(&self) -> NonZeroUsize {
        self.inner.cap()
    }

    /// R1521 §5.36 — the ceiling [`Self::capacity`] will not pass.
    #[must_use]
    pub fn max_capacity(&self) -> NonZeroUsize {
        self.max_capacity
    }

    /// R1521 §5.36 §5.7 — every counter this cache keeps, as one `Copy`
    /// snapshot for the §2 #2 wire (`scene/text_cache_stats`).
    ///
    /// The individual accessors stay because in-process callers ask one
    /// question at a time; this exists because an agent asking "is the shaper
    /// thrashing" needs several of them **from the same instant**. Reading
    /// them one call at a time across a frame boundary can report a `shapes`
    /// from before a growth beside a `capacity` from after it, which describes
    /// a cache that never existed.
    ///
    /// `usize` counters widen to `u64` here so the wire shape does not vary
    /// with the host's pointer width.
    #[must_use]
    pub fn stats(&self) -> TextCacheStats {
        TextCacheStats {
            shapes: self.shapes,
            run_builds: self.run_builds,
            entries: self.inner.len() as u64,
            capacity: self.inner.cap().get() as u64,
            max_capacity: self.max_capacity.get() as u64,
            growths: self.growths,
            font_scans: self.font_scans,
        }
    }

    /// R1521 §5.36 — how many times the capacity has doubled.
    ///
    /// The instrument that separates the two access patterns a text cache
    /// sees, which no other counter distinguishes. A **cyclic** working set
    /// larger than the capacity drives this up a few times and then stops
    /// (the set now fits); a **scan** of never-revisited strings leaves it at
    /// zero however long it runs, because a scan produces no ghost hits.
    ///
    /// `shapes()` alone cannot tell those apart — both climb by the pass size
    /// every pass — which is exactly why a cache that grew on *misses* rather
    /// than on evicted-key returns would inflate itself to the ceiling on the
    /// first streaming log it ever rendered.
    #[must_use]
    pub fn growths(&self) -> u32 {
        self.growths
    }

    /// R1448 §5.36 — whether this cache reached the platform font database.
    ///
    /// [`SystemFontStatus::NotProbed`] until something shapes (R1447 defers
    /// the scan), then `Available` or `Unavailable`. The Qt-parity condition
    /// Qt reports as a `qWarning`, here as typed data a §2 #2 agent can read —
    /// see the [`font_source`](crate::font_source) module docs.
    #[must_use]
    pub fn system_font_status(&self) -> SystemFontStatus {
        self.font_status
    }

    /// R1448 §5.36 — families this cache made selectable via
    /// [`Self::register_font_data`], in registration order without duplicates.
    ///
    /// Qt's `QFontDatabase::applicationFontFamilies(int id)` answers this per
    /// registration id; this answers it for the cache, which is the question a
    /// binding publishing its font state actually has.
    #[must_use]
    pub fn application_font_families(&self) -> &[String] {
        &self.app_families
    }

    /// R1448 §5.36 — probe the platform font database now and report the
    /// result, building the [`FontContext`] if it does not exist yet.
    ///
    /// R1447 defers the scan to the first shape, which is right for a caller
    /// that may never shape — the whole TUI path. It is wrong for a caller that
    /// must *report* the state: before this existed, a shell with no declared
    /// application font published
    /// [`NotProbed`](SystemFontStatus::NotProbed) at boot and kept saying so
    /// even after a later frame had shaped and learned the truth. A status line
    /// that reads "not-probed" on a host proven font-less is a wrong answer, not
    /// a cautious one.
    ///
    /// So a reporter calls this and pays the scan deliberately. For a GUI shell
    /// that is not extra work: any `Scene::Text` it paints reaches the shaper on
    /// the first frame, so this moves the same scan a few milliseconds earlier
    /// in exchange for a report that is true from frame one. A caller that
    /// genuinely may never shape must NOT call it — that is exactly the cost
    /// R1447 removed.
    ///
    /// Idempotent: on a cache that has already built its context this returns
    /// the recorded verdict and scans nothing, so
    /// [`Self::font_scans`] stays at 1.
    pub fn probe_system_fonts(&mut self) -> SystemFontStatus {
        let _ = ensure_font_context(
            &mut self.font_cx,
            &mut self.font_status,
            &mut self.font_scans,
        );
        self.font_status
    }

    /// R1448 §5.36 — register a font from memory and return the families it
    /// made selectable. Qt's `QFontDatabase::addApplicationFontFromData`.
    ///
    /// `data` is the bytes of a font file (TrueType / OpenType, including a
    /// collection). The returned names are usable immediately as
    /// [`PinFontFamily::Named`](pinion_core::style::FontFamily::Named) in any
    /// [`TextStyle`] this cache shapes — a registered family is matched by
    /// name before the platform database is consulted, so this works whether
    /// or not [`Self::system_font_status`] is
    /// [`Available`](SystemFontStatus::Available). On a host where it is
    /// `Unavailable`, this is how an application gets glyphs at all.
    ///
    /// Returns an empty vector if `data` is not a font pinion's shaper can
    /// read. That is a report, not a panic: the caller passed bytes from
    /// somewhere (a file, an asset bundle, an RPC payload) and a malformed
    /// asset is an ordinary runtime condition. An empty return with
    /// [`Self::application_font_families`] unchanged says precisely "nothing
    /// became selectable", which is more than Qt's `-1` sentinel carries.
    ///
    /// Registering forces the [`FontContext`] into existence, so it counts one
    /// [`Self::font_scans`] on a cache that had not shaped yet. That is not an
    /// accident of implementation — it really does pay the platform scan, and
    /// the counter's job is to report what was paid.
    ///
    /// Cached layouts are not invalidated: a name that previously resolved to
    /// a fallback keeps its already-shaped entry. Register before shaping the
    /// text that should use the face — which is what an application doing this
    /// at startup, as Qt apps do, already does.
    pub fn register_font_data(&mut self, data: Vec<u8>) -> Vec<String> {
        let cx = ensure_font_context(
            &mut self.font_cx,
            &mut self.font_status,
            &mut self.font_scans,
        );
        let registered = cx.collection.register_fonts(Blob::from(data), None);
        let mut names = Vec::with_capacity(registered.len());
        for (family_id, _faces) in registered {
            if let Some(name) = cx.collection.family_name(family_id) {
                names.push(name.to_owned());
            }
        }
        for name in &names {
            if !self.app_families.contains(name) {
                self.app_families.push(name.clone());
            }
        }
        names
    }

    /// R1472 §5.36 — set the family an unset [`TextStyle::font_family`]
    /// resolves to. Qt's `QApplication::setFont`.
    ///
    /// [`Self::register_font_data`] is only half of what an application that
    /// ships its own face needs: it makes the family selectable *by name*, and
    /// a binding still has to name it on every [`TextStyle`] it emits. Qt
    /// applications do not — they call `addApplicationFont` and then
    /// `QApplication::setFont`, and every unstyled widget follows. Without the
    /// second half, `font_family: None` means "the platform stack" and a face
    /// the application shipped is unreachable to any node that did not spell
    /// it out, so an application whose script the host has no face for renders
    /// nothing while holding the glyphs in memory. That is the state R1471
    /// measured, and it is why a layout assertion about Hangul could not be
    /// written against a host-neutral view.
    ///
    /// Only the *family* is defaultable, and that is not a shortcut: every
    /// other [`TextStyle`] field already carries a concrete value from
    /// [`TextStyle::new`], so the family is the one axis with an "unset" state
    /// to resolve. Making the rest defaultable would first mean giving them an
    /// unset state, which no consumer has asked for.
    ///
    /// # Why this clears the shape cache, when registering does not
    ///
    /// Registering a face is *additive* — it can only add a way to resolve a
    /// name nobody had resolved before, so an already-shaped entry stays
    /// correct and [`Self::register_font_data`] deliberately keeps it.
    /// Changing the default *re-interprets keys that already exist*: every
    /// cached entry whose style left the family unset was shaped against the
    /// previous answer, and the `LayoutKey` cannot tell the two apart
    /// because it holds the style as written, not as resolved. Keeping those
    /// entries would render the old family for the new default until they aged
    /// out of the LRU. So the entries go.
    ///
    /// In the shell this runs once at boot, before anything has shaped, and
    /// clears nothing.
    pub fn set_default_font_family(&mut self, family: Option<PinFontFamily>) {
        if self.default_family == family {
            return;
        }
        self.default_family = family;
        self.inner.clear();
        // R1521 — the ghosts go with them. A key evicted under the previous
        // default is no longer evidence that the capacity is too small: the
        // caller asking for it again is asking for a *different* layout, and
        // counting it as a witness would grow the cache for a reason that has
        // nothing to do with the working set.
        self.ghosts.clear();
    }

    /// R1472 §5.36 — the family an unset [`TextStyle::font_family`] resolves
    /// to, or `None` when unset styles keep the platform stack.
    #[must_use]
    pub fn default_font_family(&self) -> Option<&PinFontFamily> {
        self.default_family.as_ref()
    }

    /// R1002 §5.41 — measure the resolved monospace cell at `font_size_px`.
    ///
    /// This is the real Vello font measurement the R968 `CellMetric::new`
    /// font-derivation hook documented as deferred ("a real Vello font
    /// measurement lands later"). It shapes a probe glyph in the generic
    /// `monospace` family at `font_size_px` and reads two facts off the
    /// resolved fixed-pitch face:
    ///
    /// - **cell width** = the glyph pen advance ([`Layout::width`] of a
    ///   single monospace glyph). A `Scene::TextGrid` whose `cell_w` is this
    ///   advance has no horizontal looseness — the glyph fills its column —
    ///   instead of the gap a guessed width leaves to the left-aligned glyph.
    /// - **cell height** = `ceil` of the font's natural line box
    ///   (`block_max_coord − block_min_coord`). Rounding *up* guarantees the
    ///   cell contains the whole line box, so painting glyphs at `font_size_px`
    ///   (the SSOT path below) never clips a descender.
    ///
    /// The result is the `CellMetric` a producer (a terminal host such as
    /// sprag) hands to [`pinion_core::scene::TextGridNode::new`] so its grid
    /// matches the rendered monospace face on both axes. The producer pairs it
    /// with [`pinion_core::scene::TextGridNode::with_font_size_px`]`(font_size_px)`
    /// so the paint adapter renders at exactly this size — then `cell_w` equals
    /// the painted advance by construction (`font_size_px` is the single
    /// font-size SSOT), not by a fit re-derivation. The measurement needs a
    /// [`FontContext`] (it shapes), so it lives here on the cache and runs once
    /// at host boot, not in the sync view fn.
    ///
    /// Returns `None` only if the probe produces a degenerate (zero) axis
    /// after rounding — never in practice for a real monospace face;
    /// callers fall back to [`pinion_core::CellMetric::DEFAULT`]. Relies on
    /// the R1002 generic-keyword routing above: without it `"monospace"`
    /// would resolve to a proportional fallback and the advance would not be
    /// a true cell width.
    #[must_use]
    pub fn measure_monospace_cell(&mut self, font_size_px: u32) -> Option<pinion_core::CellMetric> {
        let mut style = TextStyle::new().with_generic_family(GenericFontFamily::Monospace);
        style.font_size_px = font_size_px;
        // line_height stays `Normal` so the measured line box is the font's
        // natural box (parley `MetricsRelative(1.0)`), matching the paint
        // adapter's R1001 natural-box probe.
        let layout = self.layout("M", &style, None);
        // The single-glyph content width is the monospace pen advance.
        let advance = f64::from(layout.width());
        let line = layout.lines().next()?;
        let m = line.metrics();
        let line_box = f64::from(m.block_max_coord - m.block_min_coord);
        // The grid coordinate space is integral and `CellMetric` rejects a zero
        // axis. Width rounds to nearest (the conventional monospace cell width;
        // the glyph's side bearing absorbs the sub-pixel remainder). Height
        // rounds *up* (ceil) so the cell always contains the natural line box —
        // painting at `font_size_px` then never clips a descender. Both are
        // small positive px well inside u32, so the casts are exact on the
        // integer part.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "advance / line box are small positive px; round/ceil then bound by CellMetric::new"
        )]
        let cell_w = advance.round().max(0.0) as u32;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "advance / line box are small positive px; round/ceil then bound by CellMetric::new"
        )]
        let cell_h = line_box.ceil().max(0.0) as u32;
        pinion_core::CellMetric::new(cell_w, cell_h)
    }

    fn shape(
        &mut self,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> Layout {
        // R51.31 §5.37.4 — UAX #9 L4 mirroring is applied here, at the
        // cache boundary, so paint_adapter sees `cache.layout(raw)` and
        // the single LRU entry covers both the mirror substitution and
        // the parley shape pass. `mirror_paired_brackets` short-circuits
        // to `Cow::Borrowed` for LTR / bracket-free content (the common
        // case for static UI labels), so the integration is allocation-
        // free on the hot path. Pre-R51.31 the call lived in
        // `paint_adapter::paint_text` and ran per-frame even on shape
        // cache hits; folding it in here lets a cache hit skip both
        // the BIDI pipeline and the shape engine.
        let mirrored = pinion_text_unicode::bidi::mirror_paired_brackets(text);
        let shape_input = mirrored.as_ref();
        // R1447 §5.36 — the one place the system-font scan happens. Every
        // shaping entry point funnels through here, so deferring the build
        // to this line defers it for the whole surface; a caller that never
        // reaches `shape` never enumerates a font. `get_or_insert_with`
        // borrows `font_cx` and `layout_cx` as disjoint fields, so the
        // builder still takes both without a second pass over `self`.
        self.shapes += 1;
        // Bound before the `&mut` field borrows below; disjoint fields, so the
        // resolved default rides alongside them without a second pass.
        let default_family = self.default_family.as_ref();
        let font_cx = ensure_font_context(
            &mut self.font_cx,
            &mut self.font_status,
            &mut self.font_scans,
        );
        let mut builder = self
            .layout_cx
            .ranged_builder(font_cx, shape_input, 1.0, true);
        // R47.6 §5.36 — the base style is pushed as the run default
        // (the whole-string style). R713 §5.36 — each StyleRun then
        // pushes its fully-resolved style over its UTF-8 byte range;
        // parley resolves overlaps last-push-wins, so list order is the
        // run priority. `runs.is_empty()` collapses to the pre-R713
        // default-only path.
        // R1472 §5.36 — the application default reaches the base style AND
        // every styled run: a run that overrides weight while leaving the
        // family unset is as unset as the base, and resolving only the base
        // would make a bolded span of an application-font paragraph fall back
        // to the platform stack mid-line.
        for prop in style_properties(style, default_family) {
            builder.push_default(prop);
        }
        for run in runs {
            let range = run.start as usize..run.end as usize;
            for prop in style_properties(&run.style, default_family) {
                builder.push(prop, range.clone());
            }
        }
        let mut layout = builder.build(shape_input);
        #[allow(
            clippy::cast_precision_loss,
            reason = "max_width <= 2^24 px in practice"
        )]
        let break_at = max_width.map(|w| w as f32);
        layout.break_all_lines(break_at);
        layout.align(
            map_text_align(style.text_align),
            AlignmentOptions::default(),
        );
        layout
    }
}

/// R1521 §5.36 — the ghost-list identity of a [`LayoutKey`].
///
/// The ghost list answers one question — "did this cache evict this key
/// recently?" — so it stores identities rather than keys. See
/// [`LayoutCache::ghosts`] for why that is the right trade and what a
/// collision costs.
///
/// `DefaultHasher` because the value never leaves the process and never
/// crosses a run: it identifies an entry against other entries in the same
/// cache, and nothing is persisted or compared across executions.
fn ghost_hash(key: &LayoutKey) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}

/// R713 §5.36 — lower a [`TextStyle`] to the parley [`StyleProperty`]
/// set, ready to push either as the run default or over a styled-run
/// range. Pulling this out of `shape` lets the base style and every
/// [`StyleRun`] share one mapping — the styled-run path is exactly the
/// single-style path applied over a sub-range.
///
/// The returned properties own their data (`'static`), so the same
/// list pushes via `push_default` (range = whole text) or `push(_,
/// range)` (a run) without lifetime juggling.
fn style_properties(
    style: &TextStyle,
    default_family: Option<&PinFontFamily>,
) -> Vec<StyleProperty<'static, Color>> {
    // u32 → f32: font_size_px fits f32 mantissa losslessly up to 2^24
    // px, far beyond any realistic UI font size.
    #[allow(
        clippy::cast_precision_loss,
        reason = "font_size_px <= 2^24 px in practice"
    )]
    let font_size = style.font_size_px as f32;
    // letter_spacing i32 → f32 px (signed). Realistic UI ranges
    // (-32..=32) fit f32 exactly; the cast is loss-free.
    #[allow(
        clippy::cast_precision_loss,
        reason = "letter_spacing |v| <= 2^24 in practice"
    )]
    let letter_spacing_px = style.letter_spacing as f32;
    let mut props = vec![
        StyleProperty::FontSize(font_size),
        StyleProperty::Brush(style.fg_color),
        StyleProperty::FontWeight(parley::FontWeight::new(f32::from(style.font_weight.0))),
        StyleProperty::FontStyle(map_font_style(style.font_style)),
        StyleProperty::LineHeight(map_line_height(style.line_height)),
        StyleProperty::LetterSpacing(letter_spacing_px),
        // R1540 — parley is asked for the underline's METRICS and BRUSH; the
        // FORM (single / double / curly / dotted / dashed) is pinion's and is
        // resolved by `glyph_run::positioned_runs`, because parley has no
        // notion of an undercurl.
        StyleProperty::Underline(style.decoration.underline.is_on()),
        StyleProperty::UnderlineBrush(style.decoration.underline_color),
        StyleProperty::Strikethrough(style.decoration.strikethrough),
    ];
    // R47.6 — pinned font family override. R1472 §5.36 — an unset family is
    // resolved against the application default (`LayoutCache::
    // set_default_font_family`, Qt's `QApplication::setFont`) before it is
    // allowed to mean "parley's platform stack"; with no default declared the
    // two are the same thing and this is byte-identical to pre-R1472.
    // R1002 §5.36 — the family is a typed
    // [`PinFontFamily`]: a CSS *generic* class routes through
    // `FontFamilyName::Generic` (a generic resolves to a real face of its
    // class — `monospace` → a fixed-pitch font — and needs no extra fallback),
    // while a *named* family is matched by name with a `SansSerif` generic
    // fallback so a missing name does not render a "tofu" run. The named-vs-
    // generic decision is carried by the type (decided once at construction /
    // wire ingest), not re-parsed from a string here each shape pass.
    if let Some(family) = style.font_family.as_ref().or(default_family) {
        let families: Vec<FontFamilyName<'static>> = match family {
            PinFontFamily::Generic(g) => vec![FontFamilyName::Generic(map_generic_family(*g))],
            PinFontFamily::Named(name) => vec![
                FontFamilyName::Named(Cow::Owned(name.as_ref().to_owned())),
                FontFamilyName::Generic(GenericFamily::SansSerif),
            ],
        };
        props.push(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(
            families,
        ))));
    }
    props
}

/// R1002 §5.36 — pinion [`GenericFontFamily`] → parley [`GenericFamily`]. The
/// one place the two parallel generic enums are bridged (pinion-core stays
/// parley-free; pinion-text owns the wiring). The variants correspond 1:1.
fn map_generic_family(g: GenericFontFamily) -> GenericFamily {
    match g {
        GenericFontFamily::Serif => GenericFamily::Serif,
        GenericFontFamily::SansSerif => GenericFamily::SansSerif,
        GenericFontFamily::Monospace => GenericFamily::Monospace,
        GenericFontFamily::Cursive => GenericFamily::Cursive,
        GenericFontFamily::Fantasy => GenericFamily::Fantasy,
        GenericFontFamily::SystemUi => GenericFamily::SystemUi,
        GenericFontFamily::UiSerif => GenericFamily::UiSerif,
        GenericFontFamily::UiSansSerif => GenericFamily::UiSansSerif,
        GenericFontFamily::UiMonospace => GenericFamily::UiMonospace,
        GenericFontFamily::UiRounded => GenericFamily::UiRounded,
        GenericFontFamily::Emoji => GenericFamily::Emoji,
        GenericFontFamily::Math => GenericFamily::Math,
        GenericFontFamily::FangSong => GenericFamily::FangSong,
    }
}

/// R47.6 — pinion `FontStyle` → parley `FontStyle`. `Oblique` widens
/// from `Option<i16>` (pinion: Hash-safe) to `Option<f32>` (parley:
/// `slnt` axis).
fn map_font_style(style: FontStyle) -> parley::FontStyle {
    match style {
        FontStyle::Italic => parley::FontStyle::Italic,
        FontStyle::Oblique(angle) => parley::FontStyle::Oblique(angle.map(f32::from)),
        // Normal + any future #[non_exhaustive] variant.
        _ => parley::FontStyle::Normal,
    }
}

/// R47.6 — pinion `LineHeight` → parley `LineHeight`. `MultiplierX100`
/// widens from fixed-point u16 (pinion: Hash-safe) to `f32`.
fn map_line_height(line_height: LineHeight) -> ParleyLineHeight {
    match line_height {
        LineHeight::Px(px) =>
        {
            #[allow(
                clippy::cast_precision_loss,
                reason = "line_height px <= 2^24 in practice"
            )]
            ParleyLineHeight::Absolute(px as f32)
        }
        LineHeight::MultiplierX100(m) => ParleyLineHeight::FontSizeRelative(f32::from(m) / 100.0),
        // Normal + any future #[non_exhaustive] variant → parley
        // default (MetricsRelative(1.0)).
        _ => ParleyLineHeight::MetricsRelative(1.0),
    }
}

/// R47.6 — pinion `TextAlign` → parley `Alignment`. Both enums share
/// the writing-mode-aware Start/End shape; Center / Justify map 1:1.
fn map_text_align(align: TextAlign) -> Alignment {
    match align {
        TextAlign::Center => Alignment::Center,
        TextAlign::End => Alignment::End,
        TextAlign::Justify => Alignment::Justify,
        // Start + any future #[non_exhaustive] variant.
        _ => Alignment::Start,
    }
}

impl Default for LayoutCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(size: u32) -> TextStyle {
        // TextStyle is #[non_exhaustive] (forward-compat for R47.x
        // font_family / weight / decoration). Build through the public
        // constructor then override the size.
        let mut s = TextStyle::new();
        s.font_size_px = size;
        s
    }

    // ---------------------------------------------------------------
    // R1531 §5.36 — the draw list is derived once per shaped layout.
    // ---------------------------------------------------------------

    /// The whole of R1531 as an assertion: the walk over parley's runs is a
    /// pure function of the layout, so painting the same text ten times must
    /// run it once.
    ///
    /// A count, not a flag, for the reason `shapes` is a count: the defect
    /// this replaces did not fail to derive, it derived every time, and only a
    /// number separates "derived once" from "derived ten times identically".
    /// The two produce the same pixels.
    #[test]
    fn r1531_the_draw_list_is_derived_once_and_replayed() {
        let mut cache = LayoutCache::new();
        let st = style(16);
        for _ in 0..10 {
            let runs = cache.positioned_runs("Row label", &st, &[], None);
            assert!(!runs.is_empty(), "the text shaped to at least one run");
        }
        assert_eq!(
            cache.run_builds(),
            1,
            "ten paints of unchanged text derive one draw list",
        );
        assert_eq!(cache.shapes(), 1, "and shape it once, as they always did");
    }

    /// R1540 §5.36 — the underline FORM is resolved the way parley resolves
    /// everything else about the same span.
    ///
    /// parley carries the underline's brush and metrics but has no notion of
    /// an undercurl, so `positioned_runs` resolves the form itself from the
    /// same `(base, runs)` the layout was shaped from — and it has to mirror
    /// parley's overlap rule exactly, which the cache documents as
    /// last-push-wins.
    ///
    /// A restatement of that rule would prove nothing, so this asserts the
    /// agreement of TWO INDEPENDENT observations of the same question: parley
    /// picks the winning span's BRUSH by its own rule, pinion picks the
    /// winning span's FORM by the mirror. If the mirror were first-wins, the
    /// run would come back with the first span's form under the second span's
    /// colour, and the two would name different winners.
    #[test]
    fn r1540_the_underline_form_and_brush_name_the_same_winning_span() {
        use pinion_core::scene::StyleRun;
        use pinion_core::style::{Color, TextDecoration, UnderlineStyle};

        const FIRST: Color = Color::rgb(0x11, 0x22, 0x33);
        const SECOND: Color = Color::rgb(0xcc, 0xbb, 0xaa);

        let base = style(16);
        let marked = |form: UnderlineStyle, colour: Color| {
            let mut st = base.clone();
            st.decoration = TextDecoration::none()
                .with_underline_style(form)
                .with_underline_color(Some(colour));
            st
        };

        // Two spans over the SAME bytes, pushed in this order. Different form
        // AND different colour, so each resolver has something to say.
        let runs = vec![
            StyleRun::new(0, 4, marked(UnderlineStyle::Dotted, FIRST)),
            StyleRun::new(0, 4, marked(UnderlineStyle::Curly, SECOND)),
        ];

        let mut cache = LayoutCache::new();
        let derived = cache.positioned_runs("word", &base, &runs, None);
        let underlined: Vec<_> = derived.iter().filter_map(|r| r.underline).collect();
        assert!(
            !underlined.is_empty(),
            "premise: the overlapped bytes carry an underline at all",
        );
        for ul in underlined {
            assert_eq!(
                ul.style,
                UnderlineStyle::Curly,
                "the LAST span pushed wins the form, mirroring parley's own \
                 overlap rule",
            );
            assert_eq!(
                ul.rule.brush, SECOND,
                "and parley, resolving independently, names the same winner — \
                 a form from one span under a colour from the other is the \
                 mirror having drifted",
            );
        }
    }

    /// The laziness is load-bearing, not a reflex: this cache has callers that
    /// shape in order to *measure* and never paint what they shaped
    /// ([`crate::LayoutCacheTextMetrics`], the caret geometry, `pinion_tui`'s
    /// cell-grid measure arm). Deriving on the miss would bill them for a list
    /// nobody replays.
    #[test]
    fn r1531_a_layout_that_is_only_measured_derives_no_draw_list() {
        let mut cache = LayoutCache::new();
        let st = style(16);
        for _ in 0..10 {
            let _ = cache.layout("Row label", &st, None);
        }
        assert_eq!(cache.shapes(), 1, "premise: it shaped, and cached");
        assert_eq!(
            cache.run_builds(),
            0,
            "a caller that never painted paid for no draw list",
        );
        // And asking for one afterwards derives it from the layout already
        // held, rather than re-shaping.
        let _ = cache.positioned_runs("Row label", &st, &[], None);
        assert_eq!((cache.shapes(), cache.run_builds()), (1, 1));
    }

    /// The draw list shares its entry's key, so changed text derives again —
    /// the property that makes it correct rather than merely cheap.
    #[test]
    fn r1531_changed_text_derives_its_own_draw_list() {
        let mut cache = LayoutCache::new();
        let st = style(16);
        let first: Vec<u32> = cache
            .positioned_runs("AAAA", &st, &[], None)
            .iter()
            .flat_map(|r| r.glyphs.iter().map(|g| g.id))
            .collect();
        let second: Vec<u32> = cache
            .positioned_runs("BBBB", &st, &[], None)
            .iter()
            .flat_map(|r| r.glyphs.iter().map(|g| g.id))
            .collect();
        assert_eq!(cache.run_builds(), 2, "two texts, two draw lists");
        assert_ne!(
            first, second,
            "and the second is the second text's glyphs, not the first's",
        );
    }

    /// The draw list shares its entry's *lifetime* too: evicting the layout
    /// evicts the list, and the entry that comes back derives again.
    ///
    /// Discriminating because a list held in a side table keyed the same way
    /// would pass every assertion above and leak here — it would answer from a
    /// map the LRU no longer bounds.
    #[test]
    fn r1531_an_evicted_entry_derives_again_when_it_returns() {
        let one = NonZeroUsize::new(1).expect("1 is non-zero");
        // Pinned capacity: the adaptive growth of R1521 exists to STOP this
        // cycle from evicting, and here the eviction is the subject.
        let mut cache = LayoutCache::with_max_capacity(one, one);
        let st = style(16);
        for _ in 0..3 {
            let _ = cache.positioned_runs("AAAA", &st, &[], None);
            let _ = cache.positioned_runs("BBBB", &st, &[], None);
        }
        assert_eq!(
            cache.run_builds(),
            6,
            "each key evicts the other, so every ask is a fresh entry and a \
             fresh draw list — a list outliving its layout would report 2",
        );
        assert_eq!(cache.shapes(), 6, "and the shaper ran the same number");
    }

    /// The derivation is *correct*, checked against an independent observation
    /// of the same layout rather than against itself: every glyph id and
    /// position the cached list carries is the one parley's own walk yields.
    ///
    /// This is the assertion that would have caught a transcription slip in
    /// the derivation — a swapped `x`/`y`, a dropped run, a glyph list built
    /// from `glyphs()` (run-relative) instead of `positioned_glyphs()`
    /// (layout-absolute). Counting derivations cannot see any of those.
    #[test]
    fn r1531_the_draw_list_is_what_parley_positions() {
        use parley::PositionedLayoutItem;
        let mut cache = LayoutCache::new();
        let st = style(16);
        let text = "Row label 42 value";
        let derived: Vec<(u32, f32, f32, f32)> = cache
            .positioned_runs(text, &st, &[], None)
            .iter()
            .flat_map(|r| {
                r.glyphs
                    .iter()
                    .map(|g| (g.id, g.x, g.y, r.font_size))
                    .collect::<Vec<_>>()
            })
            .collect();

        let layout = cache.layout(text, &st, None);
        let mut walked: Vec<(u32, f32, f32, f32)> = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    let size = run.run().font_size();
                    walked.extend(run.positioned_glyphs().map(|g| (g.id, g.x, g.y, size)));
                }
            }
        }
        assert!(!walked.is_empty(), "premise: the text shaped to glyphs");
        assert_eq!(
            derived, walked,
            "the cached draw list is the walk it replaces, glyph for glyph",
        );
    }

    /// A styled run's own brush survives into the draw list, which is what
    /// multi-colour text paints. The base run's brush is a distinct value in
    /// the same list — one layout, two brushes.
    #[test]
    fn r1531_each_run_carries_its_own_brush() {
        let mut cache = LayoutCache::new();
        let base = TextStyle::new()
            .with_size_px(16)
            .with_fg(Color::rgb(0, 0, 0));
        let red = TextStyle::new()
            .with_size_px(16)
            .with_fg(Color::rgb(255, 0, 0));
        let runs = vec![StyleRun::new(0, 1, red.clone())];
        let brushes: Vec<Color> = cache
            .positioned_runs("AB", &base, &runs, None)
            .iter()
            .map(|r| r.brush)
            .collect();
        assert!(
            brushes.contains(&red.fg_color),
            "the styled run: {brushes:?}"
        );
        assert!(
            brushes.contains(&base.fg_color),
            "the base run: {brushes:?}"
        );
    }

    /// A decoration is resolved at derivation time — the font-metric fallback
    /// applied and parley's baseline-upward offset flipped into screen space.
    ///
    /// Both were painter-side arithmetic before R1531, re-run every frame. The
    /// assertion is the sign: an underline sits *below* its baseline, so a
    /// derivation that forgot the flip would place it above and this is what
    /// says so.
    #[test]
    fn r1531_a_decoration_is_resolved_against_its_font_metrics() {
        let mut cache = LayoutCache::new();
        let mut st = style(16);
        st.decoration.underline = pinion_core::style::UnderlineStyle::Single;
        let baselines: Vec<f32> = {
            let layout = cache.layout("Row", &st, None);
            layout.lines().map(|l| l.metrics().baseline).collect()
        };
        let deco = cache
            .positioned_runs("Row", &st, &[], None)
            .iter()
            .find_map(|r| r.underline)
            .expect("the style enabled an underline, so parley reports one");
        let baseline = *baselines.first().expect("one line");
        assert!(
            deco.rule.y > baseline,
            "an underline sits below its baseline ({} vs {baseline}) — screen \
             y grows downward while parley measures the offset upward",
            deco.rule.y,
        );
        assert!(
            deco.rule.size > 0.0,
            "the font metric supplied a pen width, got {}",
            deco.rule.size,
        );
    }

    /// R1447 §5.36 — constructing a cache runs no system-font scan. The
    /// discriminating half is the second assertion: without it the test
    /// would also pass on a build where nothing ever creates a
    /// `FontContext`, so it pins *deferral*, not *absence*.
    #[test]
    fn r1447_construction_builds_no_font_context() {
        let mut cache = LayoutCache::new();
        assert_eq!(
            cache.font_scans(),
            0,
            "a fresh cache has not enumerated system fonts",
        );
        let _ = cache.layout("shape me", &style(16), None);
        assert_eq!(
            cache.font_scans(),
            1,
            "the first shape builds the font context",
        );
    }

    /// R1448 §5.36 — [`LayoutCache::probe_system_fonts`] answers before anything
    /// has shaped, and is idempotent.
    ///
    /// Written because the method's doc asserted both properties and nothing
    /// checked either: "idempotent" and "`font_scans` stays at 1" were prose. A
    /// probe that rebuilt the context per call would pay the ~25 ms platform
    /// scan on every status read a binding performs.
    #[test]
    #[ignore = "asserts the HOST has no fonts; run by tools/demos/r1447_font_free_tui.py under its font-less config"]
    fn r1460_a_zero_font_config_is_font_less_from_inside_the_process() {
        // R1460 — the INSTRUMENT a font-free demo needs, replacing one R1448
        // deliberately invalidated.
        //
        // `tools/demos/r1447_font_free_tui.py` proved its zero-font config was
        // really zero-font by requiring the parley path to FAIL there. R1448
        // then taught pinion to run font-less on purpose ("a window opens on a
        // host with no fonts"), so that failure stopped happening and the
        // demo's control started failing instead — asserting pre-R1448
        // behaviour. The fix is not to weaken the control but to measure the
        // same fact positively, which is exactly what R1448's typed status is
        // for: with no fonts reachable, a probe must answer `Unavailable`.
        //
        // `#[ignore]` because this asserts a property of the ENVIRONMENT, not
        // of the code: on a normal host (and in CI) fonts exist and the honest
        // answer is `Available`, so as a plain test it would be the host-
        // dependent assertion R1453 calls a flake. The demo runs it explicitly
        // under its own font-less config, which is where the claim is true and
        // where it is worth checking.
        let mut cache = LayoutCache::new();
        assert_eq!(
            cache.probe_system_fonts(),
            SystemFontStatus::Unavailable,
            "this process can reach no system fonts — if it can, the caller's \
             font-less configuration is not font-less and every font-free \
             claim measured under it means nothing",
        );
    }

    #[test]
    fn r1448_probe_answers_before_shaping_and_is_idempotent() {
        let mut cache = LayoutCache::new();
        assert_eq!(cache.font_scans(), 0, "premise: nothing has scanned yet");
        let first = cache.probe_system_fonts();
        assert_ne!(
            first,
            SystemFontStatus::NotProbed,
            "probing resolves the status — that is the whole point of the call",
        );
        assert_eq!(cache.font_scans(), 1, "the probe IS the one scan");

        // Idempotence, and specifically the cheap kind: same answer, no rescan.
        for _ in 0..3 {
            assert_eq!(cache.probe_system_fonts(), first, "same verdict");
        }
        assert_eq!(
            cache.font_scans(),
            1,
            "repeated probes cost nothing — a rebuild per call would pay the \
             platform scan on every status read",
        );

        // And a later shape does not scan again either: the probe already built
        // the context the shaper needs.
        let _ = cache.layout("after the probe", &style(16), None);
        assert_eq!(cache.font_scans(), 1, "shaping reuses the probe's context");
    }

    /// R1448 §5.36 — **the mechanism behind the R1448 boot-report defect**, as a
    /// test rather than as the single demo observation that exposed it.
    ///
    /// Reading the status off a cache that has not shaped yields `NotProbed`
    /// *regardless of the host* — this box has 635 fonts and still answers
    /// `NotProbed` here. That is exactly why the shell's boot report said
    /// `not-probed` forever when the application declared no font: with nothing
    /// to register, nothing shaped, so nothing had looked. The fix is not "read
    /// it later", it is [`LayoutCache::probe_system_fonts`] — reporting a fact
    /// requires looking for it.
    ///
    /// The discriminating pair is the two assertions together: same cache, same
    /// host, `NotProbed` before and a real verdict after. Either alone would
    /// also pass on a build where the status never resolved at all.
    #[test]
    fn r1448_status_is_not_probed_until_something_looks() {
        let mut cache = LayoutCache::new();
        assert_eq!(
            cache.system_font_status(),
            SystemFontStatus::NotProbed,
            "an unshaped cache reports NotProbed even on a host WITH fonts — \
             the defect the shell's boot report had",
        );
        assert_ne!(
            cache.probe_system_fonts(),
            SystemFontStatus::NotProbed,
            "and it resolves as soon as something looks",
        );
    }

    /// R1447 §5.36 — the scan happens **once**, not once per shape.
    ///
    /// Three `layout` calls (two misses and a hit) must still total one
    /// scan. This is the assertion a boolean `has_font_context` could not
    /// make: a `shape` that rebuilt the context every call would leave the
    /// flag `true` throughout and pass every other test here, while costing
    /// the ~25 ms system-font enumeration on every cache miss. That exact
    /// counterfactual was run — it passed the whole suite before this test
    /// counted, and fails now.
    #[test]
    fn r1447_font_scan_runs_once_not_per_shape() {
        let mut cache = LayoutCache::new();
        let s = style(16);
        let _ = cache.layout("a", &s, None);
        let _ = cache.layout("b", &s, None);
        let _ = cache.layout("a", &s, None);
        assert_eq!(
            cache.font_scans(),
            1,
            "one scan across two cache misses and a hit",
        );
        assert_eq!(cache.len(), 2, "two distinct entries, one font context");
    }

    /// R1447 §5.36 — the deferral does not reach past shaping into the
    /// cache's other observable behavior: a shaped layout is identical to
    /// the pre-R1447 eager-context one. Shaping the same input twice
    /// through two caches (one already warm, one fresh) must agree, which
    /// it cannot if the lazily-built context resolved a different face.
    #[test]
    fn r1447_lazy_context_shapes_identically() {
        let s = style(16);
        let mut warm = LayoutCache::new();
        let _ = warm.layout("priming", &s, None);
        let warm_width = warm.layout("Hello, world", &s, None).width();
        let mut fresh = LayoutCache::new();
        let fresh_width = fresh.layout("Hello, world", &s, None).width();
        assert!(
            (warm_width - fresh_width).abs() < f32::EPSILON,
            "same input shapes the same whether the context was just built \
             or already warm: warm={warm_width} fresh={fresh_width}",
        );
    }

    #[test]
    fn layout_produces_at_least_one_line() {
        let mut cache = LayoutCache::new();
        let layout = cache.layout("Hello", &style(16), None);
        assert!(layout.lines().count() >= 1);
    }

    #[test]
    fn repeated_layout_hits_cache() {
        let mut cache = LayoutCache::new();
        let s = style(16);
        let _ = cache.layout("Cached", &s, None);
        assert_eq!(cache.len(), 1);
        let _ = cache.layout("Cached", &s, None);
        assert_eq!(cache.len(), 1, "second call should hit cache");
    }

    #[test]
    fn different_text_creates_new_entry() {
        let mut cache = LayoutCache::new();
        let s = style(16);
        let _ = cache.layout("foo", &s, None);
        let _ = cache.layout("bar", &s, None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn different_max_width_creates_new_entry() {
        let mut cache = LayoutCache::new();
        let s = style(16);
        let _ = cache.layout("text", &s, Some(100));
        let _ = cache.layout("text", &s, Some(200));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn different_style_creates_new_entry() {
        let mut cache = LayoutCache::new();
        let _ = cache.layout("text", &style(16), None);
        let _ = cache.layout("text", &style(24), None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn r1454_shapes_counts_misses_and_a_warm_set_adds_none() {
        // R1454 — the instrument the LRU's one failure mode needs. A bounded
        // working set warms once; the second pass costs nothing.
        let mut cache = LayoutCache::new();
        let style = TextStyle::new().with_size_px(13);
        let set: Vec<String> = (0..40).map(|i| format!("row-{i}")).collect();
        for s in &set {
            let _ = cache.layout(s, &style, None);
        }
        assert_eq!(cache.shapes(), 40, "one shape per distinct string");
        for s in &set {
            let _ = cache.layout(s, &style, None);
        }
        assert_eq!(cache.shapes(), 40, "a warm set adds no shapes at all");
    }

    #[test]
    fn r1454_a_pinned_capacity_past_its_working_set_reshapes_every_pass() {
        // THE CLIFF, measured rather than asserted in prose: with a working
        // set one larger than the cache, each entry evicts the one the next
        // pass wants, so a steady-state frame pays the FULL set every time.
        //
        // R1521 — this is now what a caller gets when it PINS the bound
        // (`max == initial`), not what every caller inherits. The test keeps
        // measuring it because the behaviour is still reachable and a caller
        // choosing it should know exactly what it costs.
        let cap = NonZeroUsize::new(8).expect("8 is non-zero");
        let mut cache = LayoutCache::with_max_capacity(cap, cap);
        let style = TextStyle::new().with_size_px(13);
        let set: Vec<String> = (0..9).map(|i| format!("row-{i}")).collect();
        for pass in 1..=3 {
            for s in &set {
                let _ = cache.layout(s, &style, None);
            }
            assert_eq!(
                cache.shapes(),
                9 * pass,
                "pass {pass} re-shaped the whole set: nine strings, eight slots"
            );
        }
        assert_eq!(
            cache.growths(),
            0,
            "a pinned bound does not grow — that is what pinning means",
        );
        assert_eq!(cache.capacity(), cap, "and the capacity never moved");

        // One string fewer fits, and the very next pass is free.
        let mut fits = LayoutCache::with_max_capacity(cap, cap);
        for s in &set[..8] {
            let _ = fits.layout(s, &style, None);
        }
        for s in &set[..8] {
            let _ = fits.layout(s, &style, None);
        }
        assert_eq!(fits.shapes(), 8, "eight strings in eight slots stay warm");
    }

    /// R1521 §5.36 — **the defect**, and the reason the capacity adapts: an
    /// unpinned cache whose cyclic working set is larger than its capacity
    /// grows until the set fits, and then stops shaping entirely.
    ///
    /// The discriminating assertion is the last one. Before R1521 this same
    /// set re-shaped all nine strings on every pass forever — a 0% hit rate,
    /// not a degraded one — because LRU evicts precisely the entry a cycle is
    /// about to ask for next.
    #[test]
    fn r1521_a_cyclic_working_set_grows_the_cache_until_it_fits() {
        let cap = NonZeroUsize::new(8).expect("8 is non-zero");
        let mut cache = LayoutCache::with_capacity(cap);
        let style = TextStyle::new().with_size_px(13);
        let set: Vec<String> = (0..9).map(|i| format!("row-{i}")).collect();

        for _ in 0..6 {
            for s in &set {
                let _ = cache.layout(s, &style, None);
            }
        }
        assert!(
            cache.growths() > 0,
            "the cycle came back for keys the cache had evicted — that is the \
             witness the capacity was too small",
        );
        assert!(
            cache.capacity().get() >= set.len(),
            "it grew until the working set fits: capacity={} set={}",
            cache.capacity(),
            set.len(),
        );

        // Now warm: further passes cost nothing at all.
        let settled = cache.shapes();
        for _ in 0..3 {
            for s in &set {
                let _ = cache.layout(s, &style, None);
            }
        }
        assert_eq!(
            cache.shapes(),
            settled,
            "three more passes over a set that now fits add ZERO shapes; \
             pre-R1521 they added 27",
        );
    }

    /// R1521 §5.36 — growth is **proportionate**: a working set slightly past
    /// the capacity settles one doubling up, not at the ceiling.
    ///
    /// The counterfactual is a defect this round shipped and then fixed. A
    /// `grow` that does not retire the ghost list spends its evidence one
    /// evicted key at a time: pass two of a 300-leaf scene begins with 44
    /// ghosts and grows on each of them, so the cache lands at 8,192 — 27x the
    /// working set — while every other assertion here still passes, because
    /// every other assertion is satisfied by a cache that is merely *large
    /// enough*. Only the settled capacity distinguishes "grew until it fit"
    /// from "grew until it ran out of room to grow".
    #[test]
    fn r1521_growth_settles_at_the_working_set_not_at_the_ceiling() {
        let mut cache = LayoutCache::new();
        let style = TextStyle::new().with_size_px(13);
        let set: Vec<String> = (0..300).map(|i| format!("row-{i}")).collect();
        for _ in 0..6 {
            for s in &set {
                let _ = cache.layout(s, &style, None);
            }
        }
        assert_eq!(
            cache.capacity().get(),
            512,
            "300 leaves need one doubling from 256 — not five to the ceiling",
        );
        assert_eq!(cache.growths(), 1, "and exactly one growth was justified");
        assert!(
            cache.capacity() < LayoutCache::MAX_CAPACITY,
            "a scene this size must not reach the ceiling",
        );
    }

    /// R1521 §5.36 — a **scan** does not grow the cache, however long it runs.
    ///
    /// This is the half that makes the growth rule a rule rather than a leak.
    /// A streaming log renders lines that are never revisited: every access is
    /// a miss, so a cache that grew on *misses* would inflate straight to its
    /// ceiling on content that could never have been cached usefully. Growing
    /// on evicted-key RETURNS instead distinguishes the two, because a scan
    /// produces none.
    ///
    /// The counterfactual is the paired test above: same capacity, same number
    /// of shapes, opposite verdict. Only the revisiting tells them apart.
    #[test]
    fn r1521_a_scan_never_grows_the_cache() {
        let cap = NonZeroUsize::new(8).expect("8 is non-zero");
        let mut cache = LayoutCache::with_capacity(cap);
        let style = TextStyle::new().with_size_px(13);
        for i in 0..500 {
            let _ = cache.layout(&format!("log line {i}"), &style, None);
        }
        assert_eq!(
            cache.shapes(),
            500,
            "premise: every line missed — a scan is all misses by definition",
        );
        assert_eq!(
            cache.growths(),
            0,
            "and none of those 500 misses is evidence of undersizing: no line \
             was ever asked for twice",
        );
        assert_eq!(cache.capacity(), cap, "so the capacity did not move");
    }

    /// R1521 §5.36 — growth stops at the stated ceiling.
    ///
    /// Without a bound the growth rule is an unbounded allocation driven by
    /// the access pattern, which is a worse failure than the one it fixes.
    #[test]
    fn r1521_growth_stops_at_max_capacity() {
        let start = NonZeroUsize::new(2).expect("2 is non-zero");
        let max = NonZeroUsize::new(8).expect("8 is non-zero");
        let mut cache = LayoutCache::with_max_capacity(start, max);
        let style = TextStyle::new().with_size_px(13);
        let set: Vec<String> = (0..8).map(|i| format!("row-{i}")).collect();
        for _ in 0..8 {
            for s in &set {
                let _ = cache.layout(s, &style, None);
            }
        }
        assert_eq!(
            cache.capacity(),
            max,
            "a working set that needs the whole ceiling reaches it exactly",
        );
        assert_eq!(cache.max_capacity(), max);
        let settled = cache.shapes();
        for s in &set {
            let _ = cache.layout(s, &style, None);
        }
        assert_eq!(settled, cache.shapes(), "and it is warm there");
    }

    /// R1521 §5.36 — a working set larger than the ceiling does not grow the
    /// cache at all.
    ///
    /// Not a gap in the growth rule but a consequence of sizing the ghost list
    /// to the ceiling: evidence is retained for exactly the working sets that
    /// could be served. A set of 64 against a ceiling of 8 thrashes at any
    /// permitted capacity, so climbing to the ceiling would spend memory to
    /// change nothing — the cache stays at the size its caller asked for.
    ///
    /// Worth pinning because the tempting implementation (grow on any miss, or
    /// keep ghosts only as long as the cache) gets this case wrong in opposite
    /// directions: the first inflates to the ceiling on hopeless content, the
    /// second fails to grow for the 1,200-leaf scene that motivated the round.
    #[test]
    fn r1521_a_working_set_past_the_ceiling_does_not_grow_the_cache() {
        let start = NonZeroUsize::new(2).expect("2 is non-zero");
        let max = NonZeroUsize::new(8).expect("8 is non-zero");
        let mut cache = LayoutCache::with_max_capacity(start, max);
        let style = TextStyle::new().with_size_px(13);
        let set: Vec<String> = (0..64).map(|i| format!("row-{i}")).collect();
        for _ in 0..4 {
            for s in &set {
                let _ = cache.layout(s, &style, None);
            }
        }
        assert_eq!(
            cache.capacity(),
            start,
            "64 entries fit in no permitted capacity, so growing toward the \
             ceiling would buy nothing",
        );
        assert_eq!(cache.growths(), 0);
    }

    /// R1521 §5.36 — **the production case**, at production scale: a 1,200-leaf
    /// working set against the default 256 warms completely.
    ///
    /// The discriminating number is the ratio. A ghost list sized to the
    /// *capacity* rather than to the ceiling — the 2Q/ARC convention, and the
    /// first thing this round implemented — detects a working set up to about
    /// twice the capacity and silently fails beyond it, because the cycle's own
    /// evictions push the earlier evidence out before the cycle returns to
    /// supply it. 1,200 against 256 is 4.7x, so under that sizing this test
    /// never grows once and the whole round is inert.
    #[test]
    fn r1521_a_pro_tool_working_set_warms_at_the_default_capacity() {
        let mut cache = LayoutCache::new();
        let style = TextStyle::new().with_size_px(13);
        let set: Vec<String> = (0..1200)
            .map(|i| format!("row {i} some cell content"))
            .collect();
        for _ in 0..6 {
            for s in &set {
                let _ = cache.layout(s, &style, None);
            }
        }
        assert!(
            cache.capacity().get() >= set.len(),
            "the 30-column / 40-row data grid fits: capacity={} set={}",
            cache.capacity(),
            set.len(),
        );
        let settled = cache.shapes();
        for s in &set {
            let _ = cache.layout(s, &style, None);
        }
        assert_eq!(
            settled,
            cache.shapes(),
            "a full pass over 1,200 leaves now shapes NOTHING; before R1521 it \
             shaped all 1,200, every frame, at 27.4 ms",
        );
    }

    /// R1521 §5.36 — the default cache starts at
    /// [`LayoutCache::DEFAULT_CAPACITY`] and may reach
    /// [`LayoutCache::MAX_CAPACITY`].
    ///
    /// Pins the pair as a deliberate choice: the start is what a small app
    /// pays, the ceiling is what a pro-tool scene is allowed to prove it
    /// needs. A regression that made them equal would restore the cliff for
    /// every consumer while every other test here still passed.
    #[test]
    fn r1521_default_cache_starts_small_and_may_grow() {
        let cache = LayoutCache::new();
        assert_eq!(cache.capacity(), LayoutCache::DEFAULT_CAPACITY);
        assert_eq!(cache.max_capacity(), LayoutCache::MAX_CAPACITY);
        assert!(
            LayoutCache::MAX_CAPACITY > LayoutCache::DEFAULT_CAPACITY,
            "the default cache can grow at all — equal constants would pin \
             every consumer to the pre-R1521 cliff",
        );
        assert_eq!(cache.growths(), 0, "and it has not grown yet");
    }

    /// R1521 §5.36 — changing the application default font clears the ghost
    /// list along with the entries it invalidates.
    ///
    /// A ghost is evidence about *capacity*. After a default change, the
    /// caller asking again for a cleared key is asking for a different layout,
    /// so treating that as a capacity witness would grow the cache for a
    /// reason unrelated to the working set — on a boot sequence that sets the
    /// font once, at that.
    #[test]
    fn r1521_changing_the_default_font_retires_the_ghosts() {
        let cap = NonZeroUsize::new(2).expect("2 is non-zero");
        let mut cache = LayoutCache::with_capacity(cap);
        let s = style(16);
        // Fill and overflow so "a" is evicted into the ghost list.
        for t in ["a", "b", "c"] {
            let _ = cache.layout(t, &s, None);
        }
        assert_eq!(cache.growths(), 0, "premise: nothing has grown yet");

        cache.set_default_font_family(Some(PinFontFamily::Named("Some Face".into())));
        let _ = cache.layout("a", &s, None);
        assert_eq!(
            cache.growths(),
            0,
            "re-shaping under a NEW default is not evidence the capacity was \
             too small",
        );
    }

    #[test]
    fn capacity_evicts_oldest() {
        let cap = NonZeroUsize::new(2).expect("nonzero");
        let mut cache = LayoutCache::with_capacity(cap);
        let s = style(16);
        let _ = cache.layout("a", &s, None);
        let _ = cache.layout("b", &s, None);
        let _ = cache.layout("c", &s, None);
        assert_eq!(cache.len(), 2, "oldest entry evicted at capacity");
    }

    /// R587 §5.36 — pins the current `LayoutKey` shape: `fg_color`
    /// participates in the cache key via `TextStyle`'s `Hash + Eq`.
    /// A future shape-determinants / paint-metadata split (carried in
    /// `LayoutKey`'s doc and `[[r57-x-theme-fade-substrate]]`) will
    /// flip this assertion to `1`, at which point it should be
    /// rewritten — not silently relaxed — so the split lands as a
    /// deliberate behavior change rather than a quiet regression.
    #[test]
    fn different_fg_color_creates_new_entry() {
        let mut cache = LayoutCache::new();
        let red = TextStyle::new()
            .with_size_px(16)
            .with_fg(Color::rgb(255, 0, 0));
        let blue = TextStyle::new()
            .with_size_px(16)
            .with_fg(Color::rgb(0, 0, 255));
        let _ = cache.layout("text", &red, None);
        let _ = cache.layout("text", &blue, None);
        assert_eq!(
            cache.len(),
            2,
            "fg_color is in LayoutKey today; paint-style split is Rule of Three carry",
        );
    }

    /// R713 §5.36 — `layout_with_runs(.., &[], ..)` is the single-style
    /// fast path: it produces the same cache key as `layout(..)`, so a
    /// styled call with no runs hits the entry a plain call created.
    #[test]
    fn empty_runs_match_single_style_entry() {
        let mut cache = LayoutCache::new();
        let s = style(16);
        let _ = cache.layout("text", &s, None);
        assert_eq!(cache.len(), 1);
        let _ = cache.layout_with_runs("text", &s, &[], None);
        assert_eq!(cache.len(), 1, "empty runs reuse the single-style entry");
    }

    /// R713 §5.36 — adding a styled run changes the key, so a run-styled
    /// node is a distinct cache entry from the same text single-styled.
    #[test]
    fn styled_runs_create_distinct_entry() {
        let mut cache = LayoutCache::new();
        let base = style(16);
        let red = TextStyle::new()
            .with_size_px(16)
            .with_fg(Color::rgb(255, 0, 0));
        let _ = cache.layout("AB", &base, None);
        let _ = cache.layout_with_runs("AB", &base, &[StyleRun::new(0, 1, red)], None);
        assert_eq!(cache.len(), 2, "a styled run is a distinct cache key");
    }

    /// R713 §5.36 — the substrate guarantee: a styled run actually
    /// reaches parley and restyles its byte range. A larger font size
    /// on the first character forces parley to split the text into two
    /// glyph runs with distinct metrics, observable in the shaped
    /// `Layout` (the same per-run data the paint adapter reads).
    #[test]
    fn styled_run_splits_and_restyles() {
        use parley::PositionedLayoutItem;
        let mut cache = LayoutCache::new();
        let base = style(16);
        let big = style(40);
        let runs = vec![StyleRun::new(0, 1, big)];
        let layout = cache.layout_with_runs("AB", &base, &runs, None);
        let mut sizes: Vec<f32> = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    sizes.push(run.run().font_size());
                }
            }
        }
        assert!(
            sizes.iter().any(|s| (*s - 40.0).abs() < 0.5),
            "the styled run shaped at 40px: {sizes:?}",
        );
        assert!(
            sizes.iter().any(|s| (*s - 16.0).abs() < 0.5),
            "the base run shaped at 16px: {sizes:?}",
        );
    }

    /// R713 §5.36 — a styled run carries its own brush through to the
    /// glyph run, which is what the paint adapter lowers to a per-run
    /// Vello brush. Distinct adjacent brushes split into distinct
    /// glyph runs.
    #[test]
    fn styled_run_carries_per_run_brush() {
        use parley::PositionedLayoutItem;
        let mut cache = LayoutCache::new();
        let base = TextStyle::new()
            .with_size_px(16)
            .with_fg(Color::rgb(0, 0, 0));
        let red = TextStyle::new()
            .with_size_px(16)
            .with_fg(Color::rgb(255, 0, 0));
        let runs = vec![StyleRun::new(0, 1, red.clone())];
        let layout = cache.layout_with_runs("AB", &base, &runs, None);
        let mut brushes: Vec<Color> = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    brushes.push(run.style().brush);
                }
            }
        }
        assert!(
            brushes.contains(&red.fg_color),
            "red run brush present: {brushes:?}"
        );
        assert!(
            brushes.contains(&base.fg_color),
            "base brush present: {brushes:?}"
        );
    }

    /// Width of the single shaped glyph in `text` at `size` px in the CSS
    /// family keyword `family` (classified via [`PinFontFamily::parse_css`], so
    /// `"monospace"` resolves to the generic) — the glyph pen advance. A test
    /// helper for the monospace / proportional advance assertions below.
    fn glyph_advance(cache: &mut LayoutCache, text: &str, family: &'static str, size: u32) -> f64 {
        let mut s = TextStyle::new();
        s.font_family = Some(PinFontFamily::parse_css(family));
        s.font_size_px = size;
        f64::from(cache.layout(text, &s, None).width())
    }

    /// R1002 §5.41 — the generic `monospace` keyword resolves to a real
    /// fixed-pitch face: every glyph shares one advance. Pre-R1002 the
    /// keyword went out as a `Named("monospace")` lookup that fell back to
    /// the proportional sans-serif generic, where a narrow `i` and a wide
    /// `W` have visibly different advances — so this assertion failed and
    /// the `Scene::TextGrid` rendered in a proportional font. Guards the
    /// generic-keyword routing in `style_properties`.
    #[test]
    fn r1002_generic_monospace_keyword_is_fixed_pitch() {
        let mut cache = LayoutCache::new();
        let i = glyph_advance(&mut cache, "i", "monospace", 32);
        let m = glyph_advance(&mut cache, "M", "monospace", 32);
        let w = glyph_advance(&mut cache, "W", "monospace", 32);
        assert!(
            i > 0.0 && m > 0.0 && w > 0.0,
            "advances are positive: i={i} M={m} W={w}"
        );
        // Fixed pitch ⇒ equal advances. Allow a sub-pixel tolerance for any
        // hinting / rounding in the shaper; a proportional font's i-vs-W gap
        // is several px, far outside this.
        assert!(
            (i - w).abs() <= 0.5 && (i - m).abs() <= 0.5,
            "monospace advances must match across glyphs: i={i} M={m} W={w}",
        );
        // Discrimination: the explicit proportional generic is NOT fixed
        // pitch on this platform — `i` is visibly narrower than `W`. This
        // proves the box has a proportional face (so the equal-advance
        // result above is a real monospace resolution, not every font
        // happening to be monospace) and that the prior `Named("monospace")`
        // + sans-serif fallback would have rendered the grid proportionally.
        let prop_i = glyph_advance(&mut cache, "i", "sans-serif", 32);
        let prop_w = glyph_advance(&mut cache, "W", "sans-serif", 32);
        assert!(
            prop_w - prop_i > 2.0,
            "sans-serif must be proportional (W wider than i): i={prop_i} W={prop_w}",
        );
    }

    /// R1002 §5.41 — [`LayoutCache::measure_monospace_cell`] yields a usable
    /// `CellMetric`: positive axes, a taller-than-wide cell (the monospace
    /// advance is narrower than the line box), and a height that scales
    /// linearly with the requested font size. This is the R968
    /// font-derivation hook a `Scene::TextGrid` producer consumes.
    #[test]
    fn r1002_measure_monospace_cell_is_usable() {
        let mut cache = LayoutCache::new();
        let m16 = cache
            .measure_monospace_cell(16)
            .expect("16px monospace measures");
        let m32 = cache
            .measure_monospace_cell(32)
            .expect("32px monospace measures");
        assert!(
            m16.cell_w() > 0 && m16.cell_h() > 0,
            "positive axes: {m16:?}"
        );
        assert!(
            m16.cell_w() < m16.cell_h(),
            "a monospace cell is taller than wide: {m16:?}",
        );
        // The line box scales linearly with font size, so doubling the size
        // ~doubles the height (allow generous rounding slack on small px).
        let ratio = f64::from(m32.cell_h()) / f64::from(m16.cell_h());
        assert!(
            (1.8..=2.2).contains(&ratio),
            "cell height should scale ~2x from 16→32 px (got {ratio:.3}: {m32:?} / {m16:?})",
        );
    }

    /// R1472 §5.36 — the default is readable back, and unset by default.
    ///
    /// The second half is the load-bearing one: a cache that defaulted to
    /// *some* family would change what every pre-R1472 caller shapes.
    #[test]
    fn r1472_default_family_round_trips_and_starts_unset() {
        let mut cache = LayoutCache::new();
        assert_eq!(
            cache.default_font_family(),
            None,
            "a fresh cache resolves an unset family to the platform stack, as \
             it did before R1472",
        );
        let family = PinFontFamily::Named("Anything".into());
        cache.set_default_font_family(Some(family.clone()));
        assert_eq!(cache.default_font_family(), Some(&family));
        cache.set_default_font_family(None);
        assert_eq!(cache.default_font_family(), None, "and it clears");
    }

    /// R1472 §5.36 — changing the default drops entries shaped against the old
    /// one; setting the same default drops nothing.
    ///
    /// Cached layouts are keyed on the style **as written**, so an entry whose
    /// style left the family unset carries no trace of what that resolved to.
    /// Without the clear, such an entry would keep rendering the previous
    /// family until it aged out of the LRU — the asymmetry with
    /// [`LayoutCache::register_font_data`], which is purely additive and
    /// deliberately keeps its entries.
    ///
    /// `shapes()` is the observation: a re-shape means the entry was gone.
    /// Neither family needs to exist on this host — the key arithmetic is what
    /// is under test, and an unmatched name resolves through the same
    /// `SansSerif` fallback either way.
    #[test]
    fn r1472_changing_the_default_evicts_what_it_reinterprets() {
        let mut cache = LayoutCache::new();
        let _ = cache.layout("keyed with an unset family", &style(16), None);
        let after_first = cache.shapes();

        let _ = cache.layout("keyed with an unset family", &style(16), None);
        assert_eq!(
            cache.shapes(),
            after_first,
            "premise: the second call is a cache HIT, so a re-shape below can \
             only mean the entry was evicted",
        );

        cache.set_default_font_family(Some(PinFontFamily::Named("Some Face".into())));
        let _ = cache.layout("keyed with an unset family", &style(16), None);
        assert_eq!(
            cache.shapes(),
            after_first + 1,
            "the entry was shaped against the previous default and had to go",
        );
        let after_change = cache.shapes();

        cache.set_default_font_family(Some(PinFontFamily::Named("Some Face".into())));
        let _ = cache.layout("keyed with an unset family", &style(16), None);
        assert_eq!(
            cache.shapes(),
            after_change,
            "re-setting the SAME default reinterprets nothing, so it evicts \
             nothing — a clear-on-every-set would re-shape the whole working \
             set on a no-op boot call",
        );
    }
}
