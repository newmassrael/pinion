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
pub struct LayoutCache {
    inner: LruCache<LayoutKey, Layout>,
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
    /// R1448 — set when `font_cx` is built; `NotProbed` until then.
    font_status: SystemFontStatus,
    /// R1448 — families made selectable by [`Self::register_font_data`], in
    /// registration order, deduplicated.
    app_families: Vec<String>,
    layout_cx: LayoutContext<Color>,
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
    /// Default cache capacity (cached layouts). A `NonZeroUsize`
    /// compile-time constant so [`LayoutCache::new`] needs no runtime
    /// unwrap.
    pub const DEFAULT_CAPACITY: NonZeroUsize = NonZeroUsize::new(256).expect("256 is non-zero");

    /// Construct a cache with `capacity` slots. Use [`LayoutCache::new`]
    /// for the default.
    #[must_use]
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        Self {
            inner: LruCache::new(capacity),
            font_cx: None,
            font_scans: 0,
            shapes: 0,
            font_status: SystemFontStatus::NotProbed,
            app_families: Vec::new(),
            layout_cx: LayoutContext::new(),
        }
    }

    /// Construct a cache with [`Self::DEFAULT_CAPACITY`] slots.
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
        if !self.inner.contains(&key) {
            let layout = self.shape(text, style, runs, max_width);
            self.inner.put(key.clone(), layout);
        }
        self.inner
            .get(&key)
            .expect("entry just inserted on cache miss")
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
    /// exceeds [`Self::DEFAULT_CAPACITY`] sees it climb by the full set size
    /// on every pass, because each entry evicts the one the next pass wants.
    ///
    /// Why that matters, measured (release, this machine, short labels):
    /// a shape **miss costs 18.5 us** and a **hit 118 ns** — 157x. So a pass
    /// over 300 strings that thrashes costs **5.6 ms**, a third of a 60fps
    /// frame, *every frame*; at 1000 strings it is over budget on its own.
    /// A cache is therefore not by itself a strategy — a per-pass working set
    /// has to be BOUNDED, which is what Qt's
    /// `QHeaderView::resizeContentsPrecision` bounds.
    ///
    /// Cheap to read (a field), so a consumer can gate a debug assertion on
    /// it or a profiler can sample it per frame.
    #[must_use]
    pub fn shapes(&self) -> u64 {
        self.shapes
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
        for prop in style_properties(style) {
            builder.push_default(prop);
        }
        for run in runs {
            let range = run.start as usize..run.end as usize;
            for prop in style_properties(&run.style) {
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

/// R713 §5.36 — lower a [`TextStyle`] to the parley [`StyleProperty`]
/// set, ready to push either as the run default or over a styled-run
/// range. Pulling this out of `shape` lets the base style and every
/// [`StyleRun`] share one mapping — the styled-run path is exactly the
/// single-style path applied over a sub-range.
///
/// The returned properties own their data (`'static`), so the same
/// list pushes via `push_default` (range = whole text) or `push(_,
/// range)` (a run) without lifetime juggling.
fn style_properties(style: &TextStyle) -> Vec<StyleProperty<'static, Color>> {
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
        StyleProperty::Underline(style.decoration.underline),
        StyleProperty::Strikethrough(style.decoration.strikethrough),
    ];
    // R47.6 — pinned font family override; `None` keeps parley's default font
    // stack (system fallback). R1002 §5.36 — the family is a typed
    // [`PinFontFamily`]: a CSS *generic* class routes through
    // `FontFamilyName::Generic` (a generic resolves to a real face of its
    // class — `monospace` → a fixed-pitch font — and needs no extra fallback),
    // while a *named* family is matched by name with a `SansSerif` generic
    // fallback so a missing name does not render a "tofu" run. The named-vs-
    // generic decision is carried by the type (decided once at construction /
    // wire ingest), not re-parsed from a string here each shape pass.
    if let Some(family) = style.font_family.as_ref() {
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
    fn r1454_a_working_set_past_capacity_reshapes_every_pass() {
        // THE CLIFF, measured rather than asserted in prose: with a working
        // set one larger than the cache, each entry evicts the one the next
        // pass wants, so a steady-state frame pays the FULL set every time.
        // This is why a content-measuring consumer needs a bound on how much
        // it measures per pass, not merely a cache.
        let cap = NonZeroUsize::new(8).expect("8 is non-zero");
        let mut cache = LayoutCache::with_capacity(cap);
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
        // One string fewer fits, and the very next pass is free.
        let mut fits = LayoutCache::with_capacity(cap);
        for s in &set[..8] {
            let _ = fits.layout(s, &style, None);
        }
        for s in &set[..8] {
            let _ = fits.layout(s, &style, None);
        }
        assert_eq!(fits.shapes(), 8, "eight strings in eight slots stay warm");
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
}
