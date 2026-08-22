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

use crate::glyph_run::{self, PositionedRun, TextBackground};
use crate::layout::Layout;
use lru::LruCache;
use parley::fontique::Blob;
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, GenericFamily,
    IndentOptions, LayoutContext, LineHeight as ParleyLineHeight, StyleProperty,
};
use pinion_core::reactive::SystemFontStatus;
use pinion_core::scene::StyleRun;
use pinion_core::style::{
    Color, FontFamily as PinFontFamily, FontStyle, GenericFontFamily, LineHeight, TextAlign,
    TextStyle,
};
use pinion_core::text_cache_stats::TextCacheStats;
use pinion_core::text_elide::{ELLIPSIS, ElideRequest, Elision, elide_to_fit};
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
/// R1546 — `backgrounds` is lazy for the same reason and separately from
/// `runs`: a text that declares no background derives an empty list once and a
/// painter that never asks derives nothing. Two `Option`s rather than one
/// bundled draw list because the two have different callers — the §7 wire asks
/// for bands without painting, and every measure-only caller asks for neither.
struct CachedLayout {
    layout: Layout,
    /// R1654 §5.36 §2 #7 — the string this entry was SHAPED from, when the
    /// overflow policy shortened it, and `None` when the authored string is
    /// what gets painted.
    ///
    /// Held here rather than recomputed because the shaped layout indexes into
    /// it: a caller reading cluster byte ranges against the authored string
    /// after an elision reads the wrong characters, which is how the first
    /// version of this test suite failed. It is also the answer
    /// `scene/text_painted` publishes, so what a reader sees is on the wire
    /// instead of only in a frame buffer.
    painted: Option<String>,
    runs: Option<Vec<PositionedRun>>,
    backgrounds: Option<Vec<TextBackground>>,
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
/// Growth is bounded by [`Self::MAX_CAPACITY`], **in entries**. R1521 read
/// that as a memory bound by multiplying it by a measured *average* entry
/// (~3.1 KB, a 24-character label over an RSS delta) to get "~26 MB", and
/// R1550 retired that reading: an average bounds nothing, because one entry
/// holding a 10,000-character paragraph with its cached draw list breaks it
/// on its own. What this cache is holding is now a **measurement** the caller
/// reads — [`Footprint::footprint`](pinion_core::footprint::Footprint), and
/// `scene/memory` over the wire — so the entry ceiling is what it says it is
/// and the byte cost is a fact rather than a product of two estimates.
///
/// A caller that needs a hard bound states one with
/// [`Self::with_max_capacity`]; that is the only way to get the old
/// fixed-ceiling behaviour back, and it is now a decision rather than a
/// default. Note it is still a bound in *entries*: this cache has no byte
/// budget, unlike `pinion_runtime::image_cache::ImageCache`.
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
    /// R1546 §5.36 — how many times a shaped layout's background bands have
    /// been derived. Reported by [`Self::background_builds`]. The `run_builds`
    /// sibling, for the same reason: replay is indistinguishable from rebuild
    /// by looking at the pixels.
    background_builds: u64,
    /// R1448 — set when `font_cx` is built; `NotProbed` until then.
    font_status: SystemFontStatus,
    /// R1448 — families made selectable by [`Self::register_font_data`], in
    /// registration order, deduplicated.
    app_families: Vec<String>,
    /// R1472 §5.36 — the family an unset [`TextStyle::font_family`] resolves
    /// to; `None` keeps parley's platform stack. See
    /// [`Self::set_default_font_family`].
    default_family: Option<PinFontFamily>,
    /// R1573 §5.36 — whether this cache is allowed to consult the platform font
    /// database at all. `false` for [`Self::with_own_fonts`]: the context is
    /// built with the scan off, so only registered faces exist.
    ///
    /// A field rather than a constructor-time decision because `font_cx` is
    /// built lazily (R1447): the answer has to survive until the first shape.
    system_fonts: bool,
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

/// R1546 §5.36 — derive `key`'s background bands if it has none, counting the
/// derivation, and return them.
///
/// Same shape as [`ensure_runs`] and for the same borrow reason. `key` must
/// already be present ([`LayoutCache::ensure_entry`] is the precondition).
fn ensure_backgrounds<'a>(
    inner: &'a mut LruCache<LayoutKey, CachedLayout>,
    background_builds: &mut u64,
    key: &LayoutKey,
) -> &'a [TextBackground] {
    let entry = inner
        .get_mut(key)
        .expect("entry just inserted on cache miss");
    if entry.backgrounds.is_none() {
        *background_builds += 1;
        entry.backgrounds = Some(glyph_run::backgrounds(
            &entry.layout,
            &key.style,
            &key.runs,
            key.text.len(),
        ));
    }
    entry
        .backgrounds
        .as_deref()
        .expect("derived above if absent")
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
    system_fonts: bool,
) -> &'a mut FontContext {
    if slot.is_none() {
        // R1573 — an own-fonts cache never probes, so it counts no scan and
        // stays `NotProbed`: nothing was asked of the platform, so neither
        // `Available` nor `Unavailable` is true of it.
        let (cx, probed) = if system_fonts {
            let (cx, probed) = crate::font_source::build_font_context();
            *scans += 1;
            (cx, probed)
        } else {
            (
                crate::font_source::own_fonts_context(),
                SystemFontStatus::NotProbed,
            )
        };
        *status = probed;
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
    /// It takes a scene of 8,192 simultaneously-painted text leaves to reach
    /// it — a dense 4K pro-tool layout (180 rows x 30 columns) is about 5,400.
    /// The bound exists so the growth rule cannot be turned into an unbounded
    /// allocation by an adversarial access pattern, not because any measured
    /// scene approaches it.
    ///
    /// **In entries, and only in entries.** R1521's doc turned this into a
    /// "~26 MB" memory bound by multiplying by an average entry; R1550 retired
    /// that (see the type doc). What the cache holds in bytes is measured, not
    /// derived, and a scene of 8,192 large entries costs far more than 26 MB.
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
            background_builds: 0,
            font_status: SystemFontStatus::NotProbed,
            app_families: Vec::new(),
            default_family: None,
            system_fonts: true,
            layout_cx: LayoutContext::new(),
        }
    }

    /// Construct a cache starting at [`Self::DEFAULT_CAPACITY`] slots.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// R1573 §5.36 — a cache that consults **only the faces it is given**: the
    /// platform font database is never scanned, so
    /// [`register_font_data`](Self::register_font_data) is the sole source of
    /// glyphs and every metric this cache measures is a function of the bytes
    /// the caller shipped.
    ///
    /// # What this closes
    ///
    /// [`register_font_data`](Self::register_font_data) (R1448) makes an
    /// application's own face *selectable*, and
    /// [`set_default_font_family`](Self::set_default_font_family) (R1472) makes
    /// it the *default* — but the platform stack is still there underneath as
    /// fallback, so a glyph the shipped face lacks silently comes from the
    /// machine and the resulting advance is a property of the machine. "My
    /// fonts are the whole font world" was inexpressible, which is a real
    /// capability gap for a kiosk, a game, a PDF exporter, and — measured at
    /// R1573 — for this crate's own tests: **40 of 94 changed their answer**
    /// when the host's font database was swapped out from under them.
    ///
    /// # What it costs, honestly
    ///
    /// Text whose script the registered faces do not cover shapes to
    /// `.notdef` (or to nothing) rather than falling back. That is the point —
    /// a silent fallback is exactly the non-determinism this constructor
    /// removes — but it means an own-fonts cache is a *decision about
    /// coverage*, not a free optimisation.
    ///
    /// [`system_font_status`](Self::system_font_status) stays
    /// [`NotProbed`](SystemFontStatus::NotProbed) forever and
    /// [`font_scans`](Self::font_scans) stays `0`: nothing was asked of the
    /// platform, so neither `Available` nor `Unavailable` is true of it. That
    /// also makes such a cache free of R1447's 25.5 ms scan.
    ///
    /// # Against the toolkit 6.11
    ///
    /// The toolkit has the two halves this builds on — `addApplicationFont` and `setFont` — and
    /// **not** this one: font database always carries the system families, `removeAllApplicationFonts()`
    /// removes only the *application's* faces, and there is no supported way
    /// to tell a font database to forget the platform's. A toolkit application
    /// cannot make its own text metrics independent of the host.
    #[must_use]
    pub fn with_own_fonts() -> Self {
        Self {
            system_fonts: false,
            ..Self::with_capacity(Self::DEFAULT_CAPACITY)
        }
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

    /// R1654 §5.36 §2 #7 — the string that is actually painted for `text`
    /// under `style` at `max_width`, or `None` when it is the authored one.
    ///
    /// The read that keeps an eliding screen honest. Without it a scene reports
    /// `demo/units/1/pose` while the reader sees `demo/uni\u{2026}`, and §2 #7
    /// says the scene is the description of what is on screen. Measured on the
    /// reference toolkit: nothing there can answer this — its label returns the
    /// authored string and the elided form exists only inside the paint call.
    ///
    /// Shapes on a miss, exactly as [`Self::layout`] does, and shares the
    /// entry: asking what is painted and then painting it costs one shape.
    ///
    /// # Panics
    ///
    /// Never panics in practice — same `LruCache` invariant as
    /// [`Self::layout`].
    pub fn painted_text(
        &mut self,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> Option<&str> {
        let key = LayoutKey {
            text: text.to_owned(),
            style: style.clone(),
            runs: runs.to_vec(),
            max_width,
        };
        self.ensure_entry(&key, text, style, runs, max_width);
        self.inner
            .get(&key)
            .expect("entry just inserted on cache miss")
            .painted
            .as_deref()
    }

    /// R1654 §5.36 §2 #7 — the INK extent of `text` under `style` at
    /// `max_width`: how wide and tall the glyphs actually are.
    ///
    /// The other half of [`Self::painted_text`], and the one that answers "does
    /// this run fit the box it was given". A rectangle in a scene is what the
    /// author PROMISED a run; this is what the shaper produced, and the two are
    /// different numbers whenever the promise was too small. Published by
    /// `scene/text_painted`, because an agent that can read a scene but cannot
    /// see pixels has no other way to know a label is spilling over its
    /// neighbour.
    ///
    /// Rounded UP: a fractional advance that a caller compared against an
    /// integer box would report a run as fitting when its last glyph is half
    /// outside.
    ///
    /// # Panics
    ///
    /// Never panics in practice — same `LruCache` invariant as
    /// [`Self::layout`].
    pub fn ink_size(
        &mut self,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> (u32, u32) {
        let layout = self.layout_with_runs(text, style, runs, max_width);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a shaped extent is a non-negative bounded pixel count"
        )]
        let size = (layout.width().ceil() as u32, layout.height().ceil() as u32);
        size
    }

    /// R1654 §5.36 — how many lines the shaper produced for `text`.
    ///
    /// Two is the number that matters: a run that wrapped put a second line
    /// where the author reserved room for one, and that line lands on whatever
    /// is below it. Published beside the ink extent so a reader can tell a
    /// wrapped run from one that is merely a pixel taller than a tight box.
    ///
    /// # Panics
    ///
    /// Never panics in practice — same `LruCache` invariant as
    /// [`Self::layout`].
    pub fn line_count(
        &mut self,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> u32 {
        u32::try_from(self.layout_with_runs(text, style, runs, max_width).len()).unwrap_or(u32::MAX)
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

    /// R1546 §5.36 — the **background bands** for `text`: the rectangles a
    /// painter fills behind the glyphs, one per visual line per declared
    /// background. Empty when nothing declares one, which is every text node
    /// that predates R1546.
    ///
    /// Same key, same entry and same eviction as [`Self::positioned_runs`] —
    /// a third derivation from the one shaped layout, cached beside the other
    /// two. Derived on the first call for an entry and reused after, which is
    /// what [`Self::background_builds`] reports.
    ///
    /// # Panics
    ///
    /// Never panics in practice — same `LruCache` invariant as
    /// [`Self::layout`].
    pub fn backgrounds(
        &mut self,
        text: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> &[TextBackground] {
        let key = LayoutKey {
            text: text.to_owned(),
            style: style.clone(),
            runs: runs.to_vec(),
            max_width,
        };
        self.ensure_entry(&key, text, style, runs, max_width);
        ensure_backgrounds(&mut self.inner, &mut self.background_builds, &key)
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
            let (layout, painted) = self.shape(text, style, runs, max_width);
            // `push` reports what left. Reached only on a miss, so a returned
            // pair is always an eviction rather than a same-key replacement.
            let entry = CachedLayout {
                layout,
                painted,
                runs: None,
                backgrounds: None,
            };
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
    /// fewer, which is what the toolkit's `resizeContentsPrecision`
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

    /// R1546 §5.36 — how many times background bands have been derived by
    /// [`Self::backgrounds`].
    ///
    /// The [`Self::run_builds`] sibling, and it states the same property: the
    /// bands are a pure function of a layout that did not change, so a scene
    /// painting the same highlighted text on 60 frames must move this by the
    /// number of distinct layouts, not by 60 times that.
    #[must_use]
    pub fn background_builds(&self) -> u64 {
        self.background_builds
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
            background_builds: self.background_builds,
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
    /// [`SystemFontStatus::NotProbed`] until something shapes (R1447 defers the scan), then `Available` or `Unavailable`.
    /// The toolkit-parity condition the toolkit reports as a `qWarning`, here as
    /// typed data a §2 #2 agent can read — see the [`font_source`](crate::font_source)
    /// module docs.
    #[must_use]
    pub fn system_font_status(&self) -> SystemFontStatus {
        self.font_status
    }

    /// R1448 §5.36 — families this cache made selectable via
    /// [`Self::register_font_data`], in registration order without duplicates.
    ///
    /// The toolkit's `applicationFontFamilies(int id)` answers this per
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
            self.system_fonts,
        );
        self.font_status
    }

    /// R1448 §5.36 — register a font from memory and return the families it
    /// made selectable. The toolkit's `addApplicationFontFromData`.
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
    /// Returns an empty vector if `data` is not a font pinion's shaper can read.
    /// That is a report, not a panic: the caller passed bytes from somewhere
    /// (a file, an asset bundle, an RPC payload) and a malformed asset is an
    /// ordinary runtime condition. An empty return with [`Self::application_font_families`] unchanged says
    /// precisely "nothing became selectable", which is more than the toolkit's
    /// `-1` sentinel carries.
    ///
    /// Registering forces the [`FontContext`] into existence, so it counts one
    /// [`Self::font_scans`] on a cache that had not shaped yet. That is not an
    /// accident of implementation — it really does pay the platform scan, and
    /// the counter's job is to report what was paid.
    ///
    /// Cached layouts are not invalidated: a name that previously resolved to
    /// a fallback keeps its already-shaped entry. Register before shaping the
    /// text that should use the face — which is what an application doing this
    /// at startup, as the toolkit apps do, already does.
    pub fn register_font_data(&mut self, data: Vec<u8>) -> Vec<String> {
        let cx = ensure_font_context(
            &mut self.font_cx,
            &mut self.font_status,
            &mut self.font_scans,
            self.system_fonts,
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
    /// resolves to. The toolkit's `setFont`.
    ///
    /// [`Self::register_font_data`] is only half of what an application that ships its own face
    /// needs: it makes the family selectable *by name*, and a binding still
    /// has to name it on every [`TextStyle`] it emits. The toolkit applications do not
    /// — they call `addApplicationFont` and then `setFont`, and every unstyled widget follows.
    /// Without the second half, `font_family: None` means "the platform stack" and a face the
    /// application shipped is unreachable to any node that did not spell it
    /// out, so an application whose script the host has no face for renders
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
    ) -> (Layout, Option<String>) {
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
        // R1654 §5.36 — **an eliding arm is single-line by construction.** A
        // paragraph that wraps has no horizontal overflow to elide: the words
        // that did not fit went to the next line rather than off the end. That
        // is CSS's rule (`text-overflow` needs `white-space: nowrap`) and the
        // reference's (its metrics helper elides one string, not a paragraph),
        // and it is why the break width is dropped here rather than honoured.
        let shortens = style.overflow.shortens();
        let break_width = if shortens { None } else { max_width };
        let layout = self.shape_plain(shape_input, style, runs, break_width);
        // The shaped layout has to be the SHORTENED one: every painter reads
        // its glyphs from here, so a policy applied at paint time would be
        // applied once per backend and once per frame.
        if let Some(cut) = self.elided_form(shape_input, style, max_width, &layout) {
            let moved = remap_runs(runs, shape_input.len(), &cut);
            let mirrored = pinion_text_unicode::bidi::mirror_paired_brackets(&cut.text);
            let shaped = self.shape_plain(mirrored.as_ref(), style, &moved, None);
            return (shaped, Some(cut.text));
        }
        (layout, None)
    }

    /// R1654 §5.36 — the string this text is painted as, when the policy
    /// shortens it and it does not fit.
    ///
    /// `None` means "paint what was authored": the policy keeps every
    /// character, or the string already fits, or there is no width to fit it
    /// into. The decision itself is [`pinion_core::text_elide::elide_to_fit`],
    /// shared with the terminal backend; what belongs here is the two things
    /// only this crate can answer — where a cut may land (parley's cluster
    /// edges, so a combining mark never leaves its base) and how wide a
    /// candidate is.
    ///
    /// The measure reads advances off `laid`, the already-shaped unwrapped
    /// line, rather than re-shaping per candidate: the search is `O(log n)`
    /// candidates and each would otherwise be a full shape.
    fn elided_form(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<u32>,
        laid: &Layout,
    ) -> Option<Elision> {
        let budget = max_width?;
        if !style.overflow.shortens() {
            return None;
        }
        let boundaries = cluster_boundaries(laid, text);
        // The ellipsis costs width of its own, and the budget the policy
        // searches against is the whole answer's, so the measure has to include
        // it — which it does, because the policy measures CANDIDATES and a
        // candidate carries the ellipsis.
        let ellipsis_px = self.advance_of(ELLIPSIS, style);
        let mut measure = |candidate: &str| {
            let marks = candidate.matches(ELLIPSIS).count();
            let body: String = candidate.replace(ELLIPSIS, "");
            let body_px = advance_between(laid, text, &body);
            body_px.saturating_add(ellipsis_px.saturating_mul(u32::try_from(marks).unwrap_or(0)))
        };
        elide_to_fit(
            &ElideRequest {
                content: text,
                boundaries: &boundaries,
                budget,
            },
            style.overflow,
            &mut measure,
        )
    }

    /// The shaped width of `piece`, in pixels, shaped on its own.
    ///
    /// Used only for the ellipsis, which is one cluster and cached by the
    /// caller's own entry the moment it is shaped.
    fn advance_of(&mut self, piece: &str, style: &TextStyle) -> u32 {
        let layout = self.shape_plain(piece, style, &[], None);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a shaped advance is a non-negative bounded pixel count"
        )]
        let width = layout.width().ceil() as u32;
        width
    }

    /// The shaping half of [`Self::shape`], without the elision pass — the
    /// recursion's base case, and the only place two shapes could disagree if
    /// they were written twice.
    fn shape_plain(
        &mut self,
        shape_input: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> Layout {
        self.shapes += 1;
        let default_family = self.default_family.as_ref();
        let font_cx = ensure_font_context(
            &mut self.font_cx,
            &mut self.font_status,
            &mut self.font_scans,
            self.system_fonts,
        );
        let mut builder = self
            .layout_cx
            .ranged_builder(font_cx, shape_input, 1.0, true);
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
            reason = "text-indent |v| <= 2^24 px in practice"
        )]
        let indent_px = style.text_indent.amount_px as f32;
        layout.set_text_indent(
            indent_px,
            IndentOptions {
                each_line: style.text_indent.each_line,
                hanging: style.text_indent.hanging,
            },
        );
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

/// R1654 — the styled spans of `runs`, moved onto the elided string.
///
/// A span whose bytes were all removed is dropped rather than clamped to an
/// empty range: an empty run would push a style over no text, which parley
/// accepts and nothing can observe, so the drop is the honest encoding.
fn remap_runs(runs: &[StyleRun], original_len: usize, cut: &Elision) -> Vec<StyleRun> {
    runs.iter()
        .filter_map(|run| {
            let (start, end) = cut.remap(original_len, run.start as usize, run.end as usize)?;
            let mut moved = run.clone();
            moved.start = u32::try_from(start).unwrap_or(u32::MAX);
            moved.end = u32::try_from(end).unwrap_or(u32::MAX);
            Some(moved)
        })
        .collect()
}

/// R1654 — the byte offsets a cut may land on: parley's cluster edges.
///
/// A cluster is what the shaper treats as indivisible, which is exactly the
/// grain a cut must respect — splitting one separates a combining mark from its
/// base, or half of a ligature from the other half.
fn cluster_boundaries(laid: &Layout, text: &str) -> Vec<usize> {
    let mut cuts = vec![0usize, text.len()];
    for line in laid.lines() {
        for item in line.items() {
            if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                let range = run.run().text_range();
                cuts.push(range.start);
                cuts.push(range.end);
                for cluster in run.run().clusters() {
                    cuts.push(cluster.text_range().start);
                    cuts.push(cluster.text_range().end);
                }
            }
        }
    }
    cuts.retain(|b| *b <= text.len() && text.is_char_boundary(*b));
    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

/// R1654 — how wide `body` is, measured against the advances of the line
/// `whole` was shaped into.
///
/// `body` is always a prefix, a suffix, or a prefix plus a suffix of `whole`,
/// because that is all an eliding policy can produce. Summing the advances of
/// the clusters it covers reuses one shape for the whole search.
fn advance_between(laid: &Layout, whole: &str, body: &str) -> u32 {
    if body.is_empty() {
        return 0;
    }
    let head = common_prefix_len(whole, body);
    let tail = common_suffix_len(&whole[head..], &body[head..]);
    let kept_start = head;
    let kept_end = whole.len() - tail;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a shaped advance is a non-negative bounded pixel count"
    )]
    let width = {
        let mut sum = 0.0f32;
        for line in laid.lines() {
            for item in line.items() {
                if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                    for cluster in run.run().clusters() {
                        let r = cluster.text_range();
                        // A cluster counts when it is inside the kept head or
                        // the kept tail.
                        let in_head = r.end <= kept_start;
                        let in_tail = r.start >= kept_end;
                        if in_head || in_tail {
                            sum += cluster.advance();
                        }
                    }
                }
            }
        }
        sum.ceil() as u32
    };
    width
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    let mut n = 0;
    for (x, y) in a.bytes().zip(b.bytes()) {
        if x != y {
            break;
        }
        n += 1;
    }
    while n > 0 && !a.is_char_boundary(n) {
        n -= 1;
    }
    n
}

fn common_suffix_len(a: &str, b: &str) -> usize {
    let mut n = 0;
    for (x, y) in a.bytes().rev().zip(b.bytes().rev()) {
        if x != y {
            break;
        }
        n += 1;
    }
    while n > 0 && !a.is_char_boundary(a.len() - n) {
        n -= 1;
    }
    n
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
    // letter_spacing → f32 px (signed). R1641: the authored value may be
    // absolute or em-relative, and `resolved_px_x100` is the ONE place that
    // difference collapses — so a caller reasoning about the resulting geometry
    // and the shaper being fed here cannot disagree about what an em is.
    //
    // Fixed point rather than a float field because `TextStyle` is `Eq + Hash`
    // for the §5.16 cache key, which a float takes away. Realistic UI ranges
    // (|v| <= 3200 hundredths) fit f32 exactly; the cast is loss-free and the
    // division by a power of ten is the only rounding in the path.
    #[allow(
        clippy::cast_precision_loss,
        reason = "resolved letter spacing |v| <= 2^24 in practice"
    )]
    let letter_spacing_px =
        style.letter_spacing.resolved_px_x100(style.font_size_px) as f32 / 100.0;
    // R1641.3 — the same resolve for the same reason, at the other gap. Two
    // fields, ONE arithmetic: an em means the same thing between clusters and
    // between words, and a second copy of the conversion is a second place for
    // that to stop being true.
    #[allow(
        clippy::cast_precision_loss,
        reason = "resolved word spacing |v| <= 2^24 in practice"
    )]
    let word_spacing_px = style.word_spacing.resolved_px_x100(style.font_size_px) as f32 / 100.0;
    let mut props = vec![
        StyleProperty::FontSize(font_size),
        StyleProperty::Brush(style.fg_color),
        StyleProperty::FontWeight(parley::FontWeight::new(f32::from(style.font_weight.0))),
        StyleProperty::FontStyle(map_font_style(style.font_style)),
        StyleProperty::LineHeight(map_line_height(style.line_height)),
        StyleProperty::LetterSpacing(letter_spacing_px),
        StyleProperty::WordSpacing(word_spacing_px),
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
    // set_default_font_family`, the toolkit's `setFont`) before it is
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
    // R1654 §5.36 — the ellipsis arms shape the SHORTENED string.
    // ---------------------------------------------------------------

    /// The characters a layout actually holds, read back off its clusters'
    /// byte ranges against the string it was shaped from.
    fn shaped_text(layout: &Layout, source: &str) -> String {
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                    for cluster in run.run().clusters() {
                        let r = cluster.text_range();
                        ranges.push((r.start, r.end));
                    }
                }
            }
        }
        ranges.sort_unstable();
        ranges.dedup();
        ranges
            .into_iter()
            .filter(|(a, b)| *b <= source.len() && a < b)
            .map(|(a, b)| &source[a..b])
            .collect()
    }

    /// ★ The arm that was declared, published on the wire and readable back
    /// off a text field since R47.5, and implemented by nobody: both painters
    /// carried a note saying they fell back to a hard cut. This is the
    /// assertion that makes the declaration true.
    #[test]
    fn r1654_an_ellipsis_arm_shapes_the_shortened_string() {
        let mut cache = crate::test_font::own_font_cache();
        let mut elided = style(12);
        elided.overflow = pinion_core::style::TextOverflow::Ellipsis;
        let content = "demo/units/1/pose";

        let full = shaped_text(cache.layout(content, &style(12), Some(60)), content);
        assert_eq!(cache.painted_text(content, &style(12), &[], Some(60)), None);
        assert_eq!(
            full, content,
            "the default arm keeps every character (and wraps them)"
        );

        let cut = cache
            .painted_text(content, &elided, &[], Some(60))
            .expect("the policy shortened it")
            .to_owned();
        assert_ne!(cut, content, "the eliding arm did something");
        assert!(cut.ends_with('\u{2026}'), "and marked the cut: {cut:?}");
        assert!(
            content.starts_with(cut.trim_end_matches('\u{2026}')),
            "what survived is a prefix of what was authored: {cut:?}"
        );
        let width = cache.layout(content, &elided, Some(60)).width();
        assert!(width <= 60.0, "and it fits its box: {width}");
    }

    /// The three eliding arms differ in which characters survive, shaped.
    #[test]
    fn r1654_the_three_arms_keep_different_ends() {
        use pinion_core::style::TextOverflow;
        let mut cache = crate::test_font::own_font_cache();
        let content = "demo/units/1/pose";
        let shaped = |cache: &mut LayoutCache, overflow| {
            let mut st = style(12);
            st.overflow = overflow;
            cache
                .painted_text(content, &st, &[], Some(70))
                .expect("all three shorten this")
                .to_owned()
        };
        let end = shaped(&mut cache, TextOverflow::Ellipsis);
        let start = shaped(&mut cache, TextOverflow::EllipsisStart);
        let middle = shaped(&mut cache, TextOverflow::EllipsisMiddle);
        assert!(
            end.ends_with('\u{2026}') && !end.starts_with('\u{2026}'),
            "{end:?}"
        );
        assert!(
            start.starts_with('\u{2026}') && !start.ends_with('\u{2026}'),
            "{start:?}"
        );
        assert!(
            middle.contains('\u{2026}')
                && !middle.starts_with('\u{2026}')
                && !middle.ends_with('\u{2026}'),
            "{middle:?}"
        );
        assert!(
            end != start && start != middle && end != middle,
            "three arms, three answers: {end:?} {start:?} {middle:?}"
        );
    }

    /// A string that fits is untouched whatever the arm — so turning the
    /// policy on costs nothing to text that never overflows.
    #[test]
    fn r1654_a_string_that_fits_is_shaped_as_authored() {
        use pinion_core::style::TextOverflow;
        let mut cache = crate::test_font::own_font_cache();
        for overflow in TextOverflow::ALL {
            let mut st = style(12);
            st.overflow = overflow;
            assert_eq!(
                shaped_text(cache.layout("ok", &st, Some(400)), "ok"),
                "ok",
                "{overflow:?}"
            );
            assert_eq!(
                cache.painted_text("ok", &st, &[], Some(400)),
                None,
                "{overflow:?}: nothing was shortened, and the answer says so"
            );
        }
    }

    /// ★ An eliding arm is single-line: the words that would have wrapped are
    /// elided instead, because a paragraph that wraps has no horizontal
    /// overflow to elide.
    #[test]
    fn r1654_an_eliding_arm_does_not_wrap() {
        let mut cache = crate::test_font::own_font_cache();
        let content = "one two three four five six seven eight";
        let wrapped = cache.layout(content, &style(12), Some(60)).len();
        let mut elided = style(12);
        elided.overflow = pinion_core::style::TextOverflow::Ellipsis;
        let lines = cache.layout(content, &elided, Some(60)).len();
        assert!(wrapped > 1, "the default arm wraps into {wrapped} lines");
        assert_eq!(lines, 1, "and the eliding arm keeps one");
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
        let mut cache = crate::test_font::own_font_cache();
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

    // ---------------------------------------------------------------
    // R1546 §5.36 — a run's declared background becomes a painted band.
    // ---------------------------------------------------------------

    const HL: pinion_core::style::Color = pinion_core::style::Color::rgb(0xFF, 0xF1, 0x76);
    const OTHER: pinion_core::style::Color = pinion_core::style::Color::rgb(0x40, 0xC0, 0x80);

    /// A run over `[start, end)` carrying `bg`, built from the base so it
    /// inherits every paragraph-level field (the authoring convention
    /// `StyleRun` documents).
    fn bg_run(base: &TextStyle, start: u32, end: u32, bg: pinion_core::style::Color) -> StyleRun {
        StyleRun::new(start, end, base.clone().with_bg_color(bg))
    }

    /// Nothing declares a background, so nothing derives one — the state every
    /// text node in the tree was in before R1546, asserted so the feature
    /// cannot start emitting bands for text that did not ask.
    #[test]
    fn r1546_undeclared_text_has_no_bands() {
        let mut cache = crate::test_font::own_font_cache();
        let st = style(16);
        assert!(cache.backgrounds("Row label", &st, &[], None).is_empty());
    }

    /// The band is cut where the DECLARATION changes, not where parley's
    /// shaping runs change.
    ///
    /// The discriminating case is a highlighted span with an unrelated style
    /// boundary inside it: `[0,9)` is highlighted and `[3,6)` is also bold, so
    /// parley shapes at least two runs across bytes it has one background for.
    /// A per-parley-run derivation yields two abutting rects (and, at f32
    /// boundaries, a hairline seam through a solid highlight); the merge yields
    /// one band whose range is the whole declaration.
    #[test]
    fn r1546_one_declaration_is_one_band_across_a_shaping_boundary() {
        let mut cache = crate::test_font::own_font_cache();
        let st = style(16);
        let mut bold = st.clone().with_bg_color(HL);
        bold.font_weight = pinion_core::style::FontWeight::BOLD;
        let runs = vec![bg_run(&st, 0, 9, HL), StyleRun::new(3, 6, bold)];

        let bands = cache.backgrounds("Row label", &st, &runs, None).to_vec();
        assert_eq!(bands.len(), 1, "one declaration, one band: {bands:?}");
        assert_eq!((bands[0].start, bands[0].end), (0, 9));
        assert_eq!(bands[0].color, HL);

        // The shaping boundary is real — the same input really does produce
        // more than one positioned run — so the merge above is doing work
        // rather than describing a layout that never split.
        let drawn = cache.positioned_runs("Row label", &st, &runs, None);
        assert!(
            drawn.len() >= 2,
            "the bold span splits the shaping; got {} run(s)",
            drawn.len()
        );
    }

    /// A `StyleRun` carries a FULLY RESOLVED style, so a run declaring no
    /// background states that its bytes have none — even inside a base style
    /// that declares one. The band list is therefore two bands with a gap,
    /// not one band with something painted over it.
    #[test]
    fn r1546_a_run_without_a_background_punches_a_hole_in_the_base() {
        let mut cache = crate::test_font::own_font_cache();
        let base = style(16).with_bg_color(HL);
        // Built from `base` then cleared — the run inherits everything else.
        let hole = StyleRun::new(3, 6, base.clone().without_bg_color());

        let bands = cache
            .backgrounds("Row label", &base, std::slice::from_ref(&hole), None)
            .to_vec();
        assert_eq!(bands.len(), 2, "the hole splits the base band: {bands:?}");
        assert_eq!((bands[0].start, bands[0].end), (0, 3));
        assert_eq!((bands[1].start, bands[1].end), (6, 9));
        assert!(
            bands[0].x + bands[0].width <= bands[1].x,
            "the two bands do not overlap: {bands:?}"
        );
    }

    /// Overlapping declarations resolve the way the shaper resolves everything
    /// else about the same bytes: last-push-wins.
    ///
    /// Asserted as an agreement between two independent observations, not as a
    /// restatement of the rule — parley picks the winning span's FOREGROUND by
    /// its own rule, and the band picks the winning span's BACKGROUND by the
    /// mirror. A first-wins mirror would make the two disagree about bytes
    /// `[4,6)`.
    #[test]
    fn r1546_overlapping_declarations_agree_with_the_shaper_on_the_winner() {
        let mut cache = crate::test_font::own_font_cache();
        let st = style(16);
        let mut first = st.clone().with_bg_color(HL);
        first.fg_color = pinion_core::style::Color::rgb(0xAA, 0x00, 0x00);
        let mut second = st.clone().with_bg_color(OTHER);
        second.fg_color = pinion_core::style::Color::rgb(0x00, 0x00, 0xBB);
        let runs = vec![StyleRun::new(0, 6, first), StyleRun::new(4, 9, second)];

        let bands = cache.backgrounds("Row label", &st, &runs, None).to_vec();
        let over_the_overlap = bands
            .iter()
            .find(|b| b.start <= 4 && b.end > 4)
            .expect("bytes 4..6 carry a background");
        assert_eq!(
            over_the_overlap.color, OTHER,
            "the LAST declaration owns the overlap: {bands:?}"
        );

        let drawn = cache.positioned_runs("Row label", &st, &runs, None);
        let ink_at_overlap = drawn
            .iter()
            .find(|r| r.glyphs.iter().any(|g| g.x >= over_the_overlap.x))
            .map(|r| r.brush);
        assert_eq!(
            ink_at_overlap,
            Some(pinion_core::style::Color::rgb(0x00, 0x00, 0xBB)),
            "parley agrees the second run won those bytes",
        );
    }

    /// A declaration that soft-wraps produces one band per visual row — the
    /// shape a highlighter pen leaves, rather than one rect bounding both rows
    /// (which would ink the empty gutter past the first row's last glyph).
    #[test]
    fn r1546_a_wrapped_declaration_is_one_band_per_visual_row() {
        let mut cache = crate::test_font::own_font_cache();
        let st = style(16);
        let text = "wrap me across two rows";
        let runs = vec![bg_run(&st, 0, u32::try_from(text.len()).unwrap(), HL)];
        let narrow = Some(90);

        let lines = cache.layout_with_runs(text, &st, &runs, narrow).len();
        assert!(lines > 1, "the width really wraps this text");

        let bands = cache.backgrounds(text, &st, &runs, narrow).to_vec();
        assert_eq!(
            bands.len(),
            lines,
            "one band per visual row: {lines} row(s), {bands:?}"
        );
        // Every band names the whole declared range — the range is what was
        // declared, the geometry is what differs per row.
        assert!(
            bands
                .iter()
                .all(|b| (b.start, b.end) == (0, u32::try_from(text.len()).unwrap()))
        );
        // Rows are stacked, not co-located.
        assert!(
            bands[1].y >= bands[0].y + bands[0].height - 1.0,
            "{bands:?}"
        );
    }

    /// The band registers with the caret's line box over the same bytes.
    ///
    /// Two INDEPENDENT observations of one question, which is what makes this
    /// worth asserting: the band comes from parley's range geometry (`selection_rects`) and
    /// the caret rect from its cursor geometry. A band derived from font
    /// metrics instead — the other defensible choice, and the one the toolkit
    /// makes — would sit a different number of pixels tall and a highlight
    /// would no longer line up with the selection drawn over it.
    #[test]
    fn r1546_the_band_registers_with_the_caret_line_box() {
        let mut cache = crate::test_font::own_font_cache();
        let st = style(16);
        let runs = vec![bg_run(&st, 0, 9, HL)];
        let bands = cache.backgrounds("Row label", &st, &runs, None).to_vec();
        assert_eq!(bands.len(), 1);

        let layout = cache.layout_with_runs("Row label", &st, &runs, None);
        let caret = crate::caret_rect_for_byte_offset(layout, 4, 1.0);
        assert!(
            (bands[0].y - caret.y).abs() < 0.5 && (bands[0].height - caret.height).abs() < 0.5,
            "band {:?} vs caret {caret:?}",
            bands[0],
        );
    }

    /// The bands are a pure function of the shaped layout, so painting the same
    /// highlighted text ten times derives them once. The `run_builds` sibling
    /// assertion, and it is a count for the same reason: replay and rebuild
    /// paint identical pixels.
    #[test]
    fn r1546_the_bands_are_derived_once_and_replayed() {
        let mut cache = crate::test_font::own_font_cache();
        let st = style(16);
        let runs = vec![bg_run(&st, 0, 3, HL)];
        for _ in 0..10 {
            assert_eq!(cache.backgrounds("Row label", &st, &runs, None).len(), 1);
        }
        assert_eq!(cache.background_builds(), 1);
        assert_eq!(cache.shapes(), 1);
        // Asking for bands does not derive the glyph draw list, and vice
        // versa: the two are separately lazy because they have separate
        // callers (the §7 wire reads bands without painting).
        assert_eq!(cache.run_builds(), 0);
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

        let mut cache = crate::test_font::own_font_cache();
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

    /// R1543 §5.39 §2 #6 — a mnemonic underlines exactly its own character and
    /// nothing else, through the real shaper.
    ///
    /// The GUI half of the backend-parity claim; the terminal half is
    /// `pinion_tui::paint::tests::r1543_a_mnemonic_underlines_only_its_own_character`.
    /// Both painters resolve the same derived [`StyleRun`] — which is why the
    /// mnemonic needed no per-backend paint code at all — but R1542 recorded
    /// that a capability added to a node has to be OBSERVED at every painter
    /// rather than inferred from sharing a field, and one of the two here is
    /// the one that runs parley.
    ///
    /// The negative half is the load-bearing one: a run spanning the whole
    /// label would satisfy "the mnemonic is underlined" and be wrong.
    #[test]
    fn r1543_a_mnemonic_underlines_only_its_own_character() {
        use pinion_core::scene::{Rect, TextNode};
        use pinion_core::style::UnderlineStyle;

        let base = style(16);
        // `Save &As` marks the `A` at byte 5 — an INTERIOR character, so a run
        // that started at 0 or covered to the end would be visible as a
        // failure rather than passing by coincidence.
        let node = TextNode::mnemonic_styled("Save &As", Rect::default(), base.clone());
        assert_eq!(node.content, "Save As");

        let mut cache = crate::test_font::own_font_cache();
        let derived = cache.positioned_runs(&node.content, &node.style, &node.runs, None);
        let underlined: Vec<_> = derived.iter().filter(|r| r.underline.is_some()).collect();
        assert_eq!(underlined.len(), 1, "exactly one run is decorated");
        let run = underlined[0];
        assert_eq!(
            run.glyphs.len(),
            1,
            "and it is ONE glyph wide — a run covering the label would still \
             satisfy `the mnemonic is underlined` and be wrong",
        );
        assert!(
            run.start_x > 0.0,
            "the rule starts inside the label (`A` of `Save As`), not at its \
             left edge, so an all-covering run cannot pass by coincidence",
        );
        assert_eq!(
            run.underline.expect("checked above").style,
            UnderlineStyle::Single,
            "a mnemonic is a plain rule, not a diagnostic squiggle",
        );
    }

    /// The laziness is load-bearing, not a reflex: this cache has callers that
    /// shape in order to *measure* and never paint what they shaped
    /// ([`crate::LayoutCacheTextMetrics`], the caret geometry, `pinion_tui`'s
    /// cell-grid measure arm). Deriving on the miss would bill them for a list
    /// nobody replays.
    #[test]
    fn r1531_a_layout_that_is_only_measured_derives_no_draw_list() {
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        // R1573 — the host path deliberately: this test's subject IS the
        // platform scan's laziness, so a cache that never scans proves nothing.
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
        //
        // R1573 — the host path deliberately, and emphatically: this asserts a
        // property of the PLATFORM database, so an own-fonts cache — which
        // never probes and answers `NotProbed` forever — would make the
        // assertion unsatisfiable. R1573's blanket migration did exactly that
        // and no local gate could see it, because `#[ignore]` keeps this out of
        // `cargo test -p pinion-text --lib`; only the demo runs it.
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
        // R1573 — the host path deliberately: probing IS the subject.
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
        // R1573 — the host path deliberately: the status under test is the
        // platform database's, and an own-fonts cache has no such status.
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
        // R1573 — the host path deliberately: counting platform scans.
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
        let mut warm = crate::test_font::own_font_cache();
        let _ = warm.layout("priming", &s, None);
        let warm_width = warm.layout("Hello, world", &s, None).width();
        let mut fresh = crate::test_font::own_font_cache();
        let fresh_width = fresh.layout("Hello, world", &s, None).width();
        assert!(
            (warm_width - fresh_width).abs() < f32::EPSILON,
            "same input shapes the same whether the context was just built \
             or already warm: warm={warm_width} fresh={fresh_width}",
        );
    }

    /// R1641.3 §5.36 — word spacing reaches the shaper, and reaches a
    /// different gap than letter spacing does.
    ///
    /// The failure this exists to catch is a field that is plumbed and inert:
    /// it round-trips through the wire, it serialises, it hashes, and nothing
    /// ever moves. So the assertions are about ADVANCE, and the sharpest one
    /// is negative — a string with no word separator must not respond to word
    /// spacing at all, which no amount of accidental plumbing produces.
    #[test]
    fn r1641_3_word_spacing_moves_only_the_word_gaps() {
        use pinion_core::style::TextSpacing;
        let mut cache = crate::test_font::own_font_cache();
        let mut base = style(16);
        base.font_size_px = 16;

        let one_em_words = {
            let mut s = base.clone();
            s.word_spacing = TextSpacing::EmX1000(1000);
            s
        };
        let one_em_letters = {
            let mut s = base.clone();
            s.letter_spacing = TextSpacing::EmX1000(1000);
            s
        };

        let plain = cache.layout("a b", &base, None).width();
        let worded = cache.layout("a b", &one_em_words, None).width();
        let lettered = cache.layout("a b", &one_em_letters, None).width();

        assert!(
            worded > plain,
            "word spacing widens a string that HAS a word gap: {plain} -> {worded}",
        );
        assert!(
            (worded - lettered).abs() > 1.0,
            "and it is not the same gap letter spacing widens: word={worded} letter={lettered}",
        );

        // The negative control. `ab` has no separator, so word spacing has
        // nothing to act on — while letter spacing still does.
        let solid = cache.layout("ab", &base, None).width();
        let solid_worded = cache.layout("ab", &one_em_words, None).width();
        let solid_lettered = cache.layout("ab", &one_em_letters, None).width();
        assert!(
            (solid_worded - solid).abs() < f32::EPSILON,
            "no separator, no effect: {solid} vs {solid_worded}",
        );
        assert!(
            solid_lettered > solid,
            "while letter spacing acts between the two clusters: {solid} -> {solid_lettered}",
        );

        // And the em unit is the font's, not a constant: the same authored
        // value at twice the size moves twice as far.
        let mut big = one_em_words.clone();
        big.font_size_px = 32;
        let mut big_plain = base.clone();
        big_plain.font_size_px = 32;
        let grew_small = worded - plain;
        let grew_big =
            cache.layout("a b", &big, None).width() - cache.layout("a b", &big_plain, None).width();
        assert!(
            grew_big > grew_small * 1.5,
            "an em follows the font size: {grew_small} at 16px, {grew_big} at 32px",
        );
    }

    /// R1641 §5.36 — which whitespace a measured box contains, stated rather
    /// than inherited.
    ///
    /// A consumer placing three `Scene::Text` leaves in one flex row to
    /// emphasise the middle span reported that the spaces between them
    /// vanished, and filed it as a documentation gap. Half of it is not: the
    /// intrinsic width this crate reports comes from parley's `width()`, which
    /// that API documents as *"the computed width of the layout **excluding**
    /// the width of trailing whitespace"* — `full_width()` is the including
    /// one. So a leaf whose content ends in a space is measured as if it did
    /// not, and the following flex item butts against the last glyph.
    ///
    /// Leading whitespace is a different fact and is measured, which is why
    /// this asserts both: the report described one symptom with two causes,
    /// and a fix aimed at `width()` would have closed only one of them.
    ///
    /// This is a REPORT, not a complaint about parley. Excluding trailing
    /// whitespace from a box is what CSS does with a flex item (white-space
    /// processing removes it) and what a text engine must do for alignment to
    /// look right. What was missing is that pinion never wrote the choice
    /// down, so the first consumer to meet it had to infer it from a gap
    /// between two words. `StyleRun` is the answer for styling inside one
    /// line — see `TextNode::runs`.
    #[test]
    fn r1641_a_measured_box_excludes_trailing_but_not_leading_space() {
        let s = style(16);
        let mut cache = crate::test_font::own_font_cache();
        let bare = cache.layout("ab", &s, None).width();
        let trailing = cache.layout("ab ", &s, None).width();
        let leading = cache.layout(" ab", &s, None).width();

        assert!(
            (trailing - bare).abs() < f32::EPSILON,
            "a trailing space does not widen the measured box: \
             bare={bare} trailing={trailing}",
        );
        assert!(
            leading > bare,
            "a leading space DOES widen it, so the two ends of one string are \
             not the same fact: bare={bare} leading={leading}",
        );
        assert!(
            cache.layout("ab ", &s, None).full_width() > bare,
            "and the trailing advance exists — `width()` declines to count it, \
             `full_width()` reports it",
        );
    }

    #[test]
    fn layout_produces_at_least_one_line() {
        let mut cache = crate::test_font::own_font_cache();
        let layout = cache.layout("Hello", &style(16), None);
        assert!(layout.lines().count() >= 1);
    }

    #[test]
    fn repeated_layout_hits_cache() {
        let mut cache = crate::test_font::own_font_cache();
        let s = style(16);
        let _ = cache.layout("Cached", &s, None);
        assert_eq!(cache.len(), 1);
        let _ = cache.layout("Cached", &s, None);
        assert_eq!(cache.len(), 1, "second call should hit cache");
    }

    #[test]
    fn different_text_creates_new_entry() {
        let mut cache = crate::test_font::own_font_cache();
        let s = style(16);
        let _ = cache.layout("foo", &s, None);
        let _ = cache.layout("bar", &s, None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn different_max_width_creates_new_entry() {
        let mut cache = crate::test_font::own_font_cache();
        let s = style(16);
        let _ = cache.layout("text", &s, Some(100));
        let _ = cache.layout("text", &s, Some(200));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn different_style_creates_new_entry() {
        let mut cache = crate::test_font::own_font_cache();
        let _ = cache.layout("text", &style(16), None);
        let _ = cache.layout("text", &style(24), None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn r1454_shapes_counts_misses_and_a_warm_set_adds_none() {
        // R1454 — the instrument the LRU's one failure mode needs. A bounded
        // working set warms once; the second pass costs nothing.
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        let cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        let mut cache = crate::test_font::own_font_cache();
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
        // R1573 — the host path deliberately: the subject is how the platform
        // resolves the GENERIC `monospace` keyword, and the only face this tree
        // vendors is proportional. Registering a monospace face would make the
        // test deterministic and stop it testing generic-family resolution.
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
        // R1573 — the host path deliberately, for the reason above.
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
        // R1573 — a bare cache deliberately: the claim is that the default
        // starts UNSET, and the deterministic fixture sets it on purpose.
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
        let mut cache = crate::test_font::own_font_cache();
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

    // ---------------------------------------------------------------
    // R1780 §5.36 — WHAT A DECLARED ALIGNMENT ALIGNS WITHIN.
    // ---------------------------------------------------------------

    /// ★★★★★ **An alignment moves a run inside the width it was given, and a
    /// width equal to the run cannot move it.**
    ///
    /// # The question this answers, open since R1695
    ///
    /// A debt has stood for 84 rounds saying `TextAlign::Center` "does nothing
    /// on an absolutely placed run", measured three times off rendered pixels:
    /// a chip's label inked at the node's left edge with centring asked for.
    /// The property survives the whole pipeline — it is published, it round
    /// trips through `scene/snapshot`, it is part of this cache's key, it is
    /// mapped to parley, and the self-hosted fast path refuses anything that is
    /// not `Start` — so the debt could only record that SOMETHING made it
    /// inert, and that "which node shape honours it" was unknown.
    ///
    /// Measured here rather than read: alignment is applied by
    /// `layout.align(..)` after `break_all_lines(break_at)`, and `break_at` is
    /// the `max_width` this cache is handed. The paint adapter hands it the
    /// node's own rectangle (`if t.rect.w > 0`). So the discriminator is NOT
    /// absolute placement — it is whether that rectangle is WIDER THAN THE
    /// TEXT. A label given a box its own size is centred in a box its own
    /// size, which is where it already was.
    ///
    /// The debt's own comparison is the same fact from the other side: the
    /// pixel rig it cites as "works" builds its node with a header's box width,
    /// and the repair that fixed the screens put the label in a flex row that
    /// is wider than it.
    #[test]
    fn r1780_an_alignment_moves_a_run_within_the_width_it_was_given() {
        use pinion_core::style::TextAlign;

        let text = "Dark";
        let base = style(14);
        let mut centred = base.clone();
        centred.text_align = TextAlign::Center;

        let mut cache = crate::test_font::own_font_cache();

        // The run's own width, which is what a caller who sized the box to the
        // text would have passed.
        let natural = {
            let runs = cache.positioned_runs(text, &base, &[], None);
            let first = runs.first().expect("the fixture font shapes this text");
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a shaped run's width in px: non-negative, far below u32::MAX"
            )]
            let w = (first.end_x - first.start_x).ceil() as u32;
            w
        };
        assert!(natural > 0, "the premise: this text inks something");

        let start_x_at = |cache: &mut LayoutCache, style: &TextStyle, w: Option<u32>| -> f32 {
            cache
                .positioned_runs(text, style, &[], w)
                .first()
                .expect("shaped")
                .start_x
        };

        // ★ A box the size of the text: centring is an IDENTITY, and this is
        // the case the debt was looking at.
        let flush_tight = start_x_at(&mut cache, &base, Some(natural));
        let centred_tight = start_x_at(&mut cache, &centred, Some(natural));
        assert!(
            (flush_tight - centred_tight).abs() < 0.5,
            "a box the width of its own text cannot move the text: \
             start {flush_tight} vs {centred_tight}",
        );

        // ★ A box wider than the text: centring MOVES it, by half the slack.
        //
        // The second assertion is the one that makes this a measurement rather
        // than a smoke test: "it moved" passes for any movement at all, and
        // only "by half the slack" says the alignment is an alignment.
        let wide = natural * 3;
        let flush_wide = start_x_at(&mut cache, &base, Some(wide));
        let centred_wide = start_x_at(&mut cache, &centred, Some(wide));
        assert!(
            centred_wide > flush_wide + 1.0,
            "given room, centring must move the run: start {flush_wide} vs \
             {centred_wide} in a box of {wide} for text of {natural}",
        );
        // Compared in f64 so no cast can lose anything: `u32 -> f64` and
        // `f32 -> f64` are both widening, so this arithmetic needs no `allow`
        // and the assertion is not weakened by the conversion's own rounding.
        let expected = f64::from(wide - natural) / 2.0;
        let moved = f64::from(centred_wide) - f64::from(flush_wide);
        assert!(
            (moved - expected).abs() < 2.0,
            "and by half the slack: moved {moved} of an expected {expected}",
        );

        // ★★ And with NO width at all the layout is its own width, so the same
        // identity holds — which is the second way a caller gets silence.
        let flush_none = start_x_at(&mut cache, &base, None);
        let centred_none = start_x_at(&mut cache, &centred, None);
        assert!(
            (flush_none - centred_none).abs() < 0.5,
            "no width means nothing to align within: {flush_none} vs {centred_none}",
        );
    }
}

/// R1550 §5.36 §5.7 — what the shape cache is holding, in bytes.
///
/// # Why this arena cannot be exact
///
/// Three of its values are parley's, and parley keeps most of their buffers
/// behind a `pub(crate)` field: `Layout`'s glyphs / clusters / runs / lines
/// (one per cache entry), the `LayoutContext` scratch space, and the
/// `FontContext`'s collection. R1550.1 narrowed this from "no API outside that
/// crate can size any of them" — a `Layout`'s `styles` and `inline_boxes` ARE
/// reachable and are now counted (see `parley_reachable_bytes`); the rest is
/// not, because the element types are `pub(crate)` as well. So the row this
/// cache publishes names them and their counts rather than reporting a total
/// that quietly omits them — see
/// [`ArenaFootprint::unmeasured`](pinion_core::memory_census::ArenaFootprint::unmeasured).
///
/// # What this replaces
///
/// [`LayoutCache::MAX_CAPACITY`]'s doc has stated a bound of "~26 MB" since
/// R1521, derived from 8,192 entries times a *measured average* entry. An
/// average is not a bound — one entry holding a 10,000-character paragraph
/// with its draw list breaks it on its own — and R1531 then widened the gap by
/// caching draw lists in entries the average predated. This is the same claim
/// as a number the caller can read.
///
/// # Cost
///
/// [`Footprint::footprint`](pinion_core::footprint::Footprint::footprint) walks every entry and every glyph in every cached
/// draw list, so it is O(glyphs) and belongs on a `scene/memory` dispatch
/// rather than in [`LayoutCache::stats`], which every RPC dispatch calls
/// unconditionally. That is why bytes are NOT a `TextCacheStats` field:
/// one method per axis, and this axis costs a walk.
mod footprint {
    use super::{CachedLayout, LayoutCache, LayoutKey};
    use pinion_core::footprint::Footprint;
    use pinion_core::memory_census::{Arena, ArenaFootprint, MeasuredArena, UnmeasuredValues};

    impl Footprint for LayoutKey {
        fn footprint(&self) -> usize {
            let Self {
                text,
                style,
                runs,
                max_width,
            } = self;
            text.footprint() + style.footprint() + runs.footprint() + max_width.footprint()
        }
    }

    /// R1550.1 §5.36 — the part of a `parley::Layout` that CAN be sized from
    /// outside parley.
    ///
    /// R1550 wrote that no API outside that crate can size any of a `Layout`'s
    /// buffers, and reported the whole value as unmeasured. **Two of them can
    /// be**: `styles()` and `inline_boxes()` hand out slices of `parley::Style`
    /// and `parley::InlineBox`, both `pub`, so `size_of` is nameable and the
    /// length is readable. The claim was checked against the crate on the round
    /// that made it and still came out one notch too strong, which is the
    /// R1537 shape — a limit recorded as external that was partly ours.
    ///
    /// What is genuinely unreachable is the rest: glyphs, clusters, runs,
    /// lines, line items, fonts and coords live in a `pub(crate) LayoutData`
    /// whose element types are `pub(crate)` too, so `size_of::<ClusterData>()`
    /// cannot even be written here. That half needs a `Layout::memory_usage()`
    /// upstream (linebender/parley), and parley has no size accessor of any
    /// kind today — censused at R1550.1.
    ///
    /// **Length, not capacity.** A slice does not report its `Vec`'s spare
    /// capacity, so this term is a floor for the two fields it covers, unlike
    /// every other [`Footprint`] impl here.
    /// The row still reports the whole `Layout` as unmeasured — this makes
    /// `bytes` less wrong, not complete.
    fn parley_reachable_bytes(layout: &super::Layout) -> usize {
        std::mem::size_of_val(layout.styles()) + std::mem::size_of_val(layout.inline_boxes())
    }

    impl Footprint for CachedLayout {
        fn footprint(&self) -> usize {
            let Self {
                layout,
                painted,
                runs,
                backgrounds,
            } = self;
            parley_reachable_bytes(layout)
                + painted.footprint()
                + runs.footprint()
                + backgrounds.footprint()
        }
    }

    impl Footprint for LayoutCache {
        fn footprint(&self) -> usize {
            let Self {
                inner,
                ghosts,
                max_capacity,
                growths,
                // parley's font collection — opaque, and named on the row.
                font_cx: _,
                font_scans,
                shapes,
                run_builds,
                background_builds,
                font_status,
                app_families,
                default_family,
                system_fonts,
                // parley's shaping scratch space — opaque, and named on the row.
                layout_cx: _,
            } = self;
            inner.footprint()
                + ghosts.footprint()
                + max_capacity.footprint()
                + growths.footprint()
                + font_scans.footprint()
                + shapes.footprint()
                + run_builds.footprint()
                + background_builds.footprint()
                + font_status.footprint()
                + app_families.footprint()
                + default_family.footprint()
                + system_fonts.footprint()
        }
    }

    impl MeasuredArena for LayoutCache {
        fn arena_footprint(&self) -> ArenaFootprint {
            let entries = self.inner.len() as u64;
            ArenaFootprint::partial(
                Arena::TextShapes,
                self.footprint() as u64,
                entries,
                vec![
                    UnmeasuredValues::new("parley::Layout", entries),
                    UnmeasuredValues::new("parley::LayoutContext", 1),
                    UnmeasuredValues::new("parley::FontContext", u64::from(self.font_cx.is_some())),
                ],
            )
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::LayoutCache;
        use pinion_core::footprint::Footprint;
        use pinion_core::memory_census::{Arena, FootprintBasis, MeasuredArena};
        use pinion_core::style::TextStyle;

        /// A fresh cache is not free — it allocates its ghost index at
        /// construction — and the arena that has shaped nothing still holds
        /// parley's scratch context, so the derived basis says `Partial`
        /// without naming a single `Layout`.
        #[test]
        fn r1550_an_empty_cache_still_holds_its_index() {
            // R1573 — `LayoutCache::new()` deliberately, not the deterministic
            // fixture: this measures an EMPTY cache, and the fixture registers a
            // face, so the fixture's cache is not the object under test.
            let cache = LayoutCache::new();
            assert!(cache.footprint() > 100_000, "{}", cache.footprint());
            let row = cache.arena_footprint();
            assert_eq!(row.entries, 0);
            assert_eq!(row.arena, Arena::TextShapes);
            assert_eq!(row.basis(), FootprintBasis::Partial);
            let named: Vec<&str> = row
                .unmeasured
                .iter()
                .filter(|u| u.count > 0)
                .map(|u| u.type_name)
                .collect();
            assert_eq!(named, ["parley::LayoutContext"]);
        }

        /// The claim the whole round rests on: entries are not the resource.
        /// Two caches holding ONE entry each, priced by their content.
        #[test]
        fn r1550_a_long_entry_costs_more_than_a_short_one() {
            let style = TextStyle::new();
            let mut short = crate::test_font::own_font_cache();
            let mut long = crate::test_font::own_font_cache();
            let empty = short.footprint();
            short.layout("OK", &style, None);
            long.layout(&"lorem ipsum dolor sit amet ".repeat(200), &style, None);
            assert_eq!(short.len(), long.len(), "one entry each");
            let (short_cost, long_cost) = (short.footprint() - empty, long.footprint() - empty);
            // The difference tracks the TEXT, byte for byte: 5,400 characters
            // of key against 2.
            assert!(
                long_cost - short_cost >= 5_000,
                "the content is what costs: {short_cost} vs {long_cost}",
            );
            // And the ratio is content-dominated rather than unbounded: an
            // entry carries a fixed ~450 bytes of LRU node and inline
            // `CachedLayout` whatever it holds, which is precisely why a
            // per-entry AVERAGE (what `MAX_CAPACITY`'s ~26 MB was derived
            // from) cannot bound anything.
            assert!(
                long_cost > 10 * short_cost,
                "one entry, 12x the bytes: {short_cost} vs {long_cost}",
            );
        }

        /// R1531's cached draw list is part of what an entry costs, and a
        /// lazily-derived one is not free the moment it arrives. Without this,
        /// dropping `runs` from the accounting would leave every other
        /// assertion in this round green.
        #[test]
        fn r1550_deriving_a_draw_list_grows_the_entry() {
            let style = TextStyle::new();
            let mut cache = crate::test_font::own_font_cache();
            let text = "the quick brown fox jumps over the lazy dog";
            cache.layout(text, &style, None);
            let shaped = cache.footprint();
            cache.positioned_runs(text, &style, &[], None);
            let with_runs = cache.footprint();
            assert_eq!(cache.len(), 1, "still one entry");
            assert!(
                with_runs > shaped,
                "the draw list is held in the entry it was derived from: \
                 {shaped} -> {with_runs}",
            );
        }

        /// R1550.1 — the reachable half of a `parley::Layout` is COUNTED.
        ///
        /// Discriminating by construction: adding style runs grows two things
        /// at once — the cache KEY (which holds the `Vec<StyleRun>`) and
        /// parley's own `styles` collection (which holds a resolved
        /// `parley::Style` per run boundary). If `parley_reachable_bytes` were
        /// dropped from the entry's accounting, the entry would grow by the
        /// key alone, so the assertion is against the key's own growth rather
        /// than against zero.
        #[test]
        fn r1550_1_the_reachable_half_of_a_parley_layout_is_counted() {
            use pinion_core::scene::StyleRun;
            let base = TextStyle::new();
            let text = "the quick brown fox jumps over the lazy dog".repeat(4);
            let mut plain = crate::test_font::own_font_cache();
            let mut styled = crate::test_font::own_font_cache();
            let empty = plain.footprint();

            plain.layout(&text, &base, None);
            // DISTINCT styles per run. An earlier draft cloned one style 32
            // times and measured a delta exactly equal to the key's growth —
            // parley resolves identical styles to a single entry, so the test
            // was asserting against a collection that had not grown. The
            // failure was the finding, not a wrong threshold.
            let runs: Vec<StyleRun> = (0..32u32)
                .map(|i| {
                    let mut style = base.clone();
                    style.font_size_px = 12 + i;
                    StyleRun::new(i * 4, i * 4 + 2, style)
                })
                .collect();
            styled.layout_with_runs(&text, &base, &runs, None);

            let key_growth = runs.footprint();
            let delta = (styled.footprint() - empty) - (plain.footprint() - empty);
            assert!(
                delta > key_growth,
                "32 style runs grow parley's own style collection as well as \
                 the key, so the entry must cost MORE than the key's own \
                 growth: delta {delta}, key {key_growth}",
            );
        }

        /// One opaque `Layout` per cached entry — the count that makes the
        /// `partial` basis an attributable limit rather than a disclaimer.
        #[test]
        fn r1550_one_opaque_layout_per_entry() {
            let style = TextStyle::new();
            let mut cache = crate::test_font::own_font_cache();
            for i in 0..7 {
                cache.layout(&format!("row {i}"), &style, None);
            }
            let row = cache.arena_footprint();
            assert_eq!(row.entries, 7);
            let layouts = row
                .unmeasured
                .iter()
                .find(|u| u.type_name == "parley::Layout")
                .expect("the opaque type is named");
            assert_eq!(layouts.count, 7);
        }
    }
}
