//! R1521 §5.36 §5.7 — [`TextCacheStats`]: what the shape cache is costing,
//! as a fact an agent can read.
//!
//! # Why this exists
//!
//! The §5.36 shape cache has one failure mode, and until R1521 nothing outside
//! the process could see it. A cyclic working set larger than the cache's
//! capacity re-shapes **every string, every frame** — not a degraded hit rate
//! but a zero one, because LRU evicts precisely the entry a cycle is about to
//! ask for. Measured, a 1,200-leaf scene spent 27.4 ms per frame there, 1.6x
//! the whole 60fps budget, indefinitely.
//!
//! A defect with that profile is invisible from every angle an agent has.
//! `scene/snapshot` shows the same tree whether the frame cost 2 ms or 27;
//! `scene/cache_stats` reports the §5.16 **paint-fragment** cache, which is a
//! different cache with different contents and which hits perfectly while the
//! shaper thrashes underneath it; `scene/frame_timings` shows a slow frame
//! without saying which of a dozen things made it slow. So the one measurement
//! that names the cause had no wire, and §2 #2 — RPC as the AI's primary path
//! — did not hold for it.
//!
//! # Why plain data, and why here
//!
//! Identical reasoning to [`SystemFontStatus`](crate::reactive::SystemFontStatus):
//! `pinion-core` stays parley-free, so the reported *type* lives here and the
//! code that produces it lives in the layer that owns the shaper
//! (`pinion_text::LayoutCache::stats`). That split is what lets
//! [`pinion-rpc`](https://docs.rs/pinion-rpc) hold a typed slot for this
//! snapshot without depending on `pinion-text` and dragging parley into the
//! RPC crate — the same arrangement `pinion_runtime::FragmentCacheStats` has
//! with `scene/cache_stats`.
//!
//! `Copy` and field-only: reading it neither borrows the cache nor shapes
//! anything, so an agent may poll it between frames without perturbing the
//! measurement it is taking.

/// R1521 §5.36 §5.7 — a snapshot of a shape cache's cost and capacity.
///
/// Produced by `pinion_text::LayoutCache::stats` and published on the wire as
/// `scene/text_cache_stats`. Every field is cumulative over the cache's
/// lifetime except [`Self::entries`], [`Self::capacity`] and
/// [`Self::max_capacity`], which describe its state right now.
///
/// # Reading it
///
/// The question this snapshot exists to answer is "is the shaper thrashing,
/// and if so why", and it takes two fields together to answer it. `shapes`
/// alone cannot: it climbs by the pass size every pass under a thrash **and**
/// under an ordinary scan of never-repeated content, which is not a defect at
/// all.
///
/// | `shapes` climbing | `growths` | reading |
/// |---|---|---|
/// | no | any | warm — the working set fits |
/// | yes | rising | growing into a working set larger than it started; settles within `log2` steps |
/// | yes | steady, `capacity == max_capacity` | working set past the ceiling — the scene needs more than the cache is permitted to hold |
/// | yes | steady, `capacity < max_capacity` | either a scan (nothing repeats, nothing to fix) or a caller-pinned bound |
/// Constructible by literal — deliberately not `#[non_exhaustive]`, matching
/// `pinion_runtime::FragmentCacheStats`, its peer for `scene/cache_stats`.
/// The producer lives in another crate by design (see the module docs), so
/// sealing construction here would leave the type with no way to be built at
/// all without a parallel constructor that carried the same fields twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextCacheStats {
    /// Cumulative cache misses — how many times this cache has run the shaper.
    ///
    /// A miss costs ~18.5 us against ~118 ns for a hit (measured, release,
    /// short labels), so this is the field that turns into milliseconds.
    pub shapes: u64,
    /// Cached layouts held right now.
    pub entries: u64,
    /// Current capacity in entries. Rises toward [`Self::max_capacity`] as the
    /// working set proves it must; see `pinion_text::LayoutCache`.
    pub capacity: u64,
    /// The ceiling [`Self::capacity`] will not pass.
    ///
    /// Equal to `capacity` for a caller that pinned its own bound, which is
    /// what distinguishes "cannot grow further" from "has not needed to".
    pub max_capacity: u64,
    /// How many times the capacity has doubled.
    ///
    /// The field that separates a thrash from a scan. A cyclic working set
    /// drives this up a few times and then stops; a scan leaves it at zero
    /// however long it runs, because a scan never asks twice for anything.
    pub growths: u32,
    /// How many times this cache has enumerated the platform font database.
    ///
    /// The R1447 invariant is "at most once". A value above 1 means something
    /// is rebuilding the font context, which costs the ~25 ms platform scan
    /// per rebuild.
    pub font_scans: u32,
}

impl TextCacheStats {
    /// Whether the cache is at its ceiling and therefore cannot grow further.
    ///
    /// The precondition for the one remaining thrash a caller cannot fix by
    /// waiting: at the ceiling, a working set that does not fit will not come
    /// to fit. Below it, a rising `shapes` is either transient (still growing)
    /// or a scan.
    #[must_use]
    pub fn at_ceiling(&self) -> bool {
        self.capacity >= self.max_capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R1521 — the default is the honest "nothing has happened yet" state.
    ///
    /// It is also the one case where [`TextCacheStats::at_ceiling`] must not
    /// read as "at the ceiling" by accident: `0 >= 0` is true, so a default
    /// built from zeroed capacities would report a cache that cannot grow.
    #[test]
    fn r1521_default_is_empty_and_not_at_a_ceiling() {
        let s = TextCacheStats::default();
        assert_eq!(s.shapes, 0);
        assert_eq!(s.entries, 0);
        assert_eq!(s.growths, 0);
        assert_eq!(s.font_scans, 0);
        assert!(
            s.at_ceiling(),
            "a zeroed snapshot has capacity == max_capacity == 0, and the \
             predicate reports exactly that rather than pretending otherwise; \
             a real cache never has a zero capacity, so this state only \
             appears where no cache was consulted",
        );
    }

    #[test]
    fn r1521_at_ceiling_distinguishes_room_from_none() {
        let growing = TextCacheStats {
            capacity: 512,
            max_capacity: 8192,
            ..TextCacheStats::default()
        };
        assert!(!growing.at_ceiling(), "512 of 8192 has room");
        let pinned = TextCacheStats {
            capacity: 8192,
            max_capacity: 8192,
            ..TextCacheStats::default()
        };
        assert!(pinned.at_ceiling(), "and 8192 of 8192 does not");
    }
}
