//! R1550 §5.16 §5.36 §5.7 §2 #2 — [`Footprint`]: the heap a live arena is
//! holding, in bytes.
//!
//! # The gap this closes
//!
//! Nothing in this tree could state its memory. A census of the RPC surface
//! at R1550 found **not one field in bytes**: `scene/cache_stats` answers with
//! `entries`, `scene/text_cache_stats` with `entries` / `capacity` /
//! `max_capacity`, `scene/frame_timings` with node counts. Every one of those
//! is a count of *things*, and a count of things is not a footprint — the
//! §5.36 shape cache holds one entry for `"OK"` and one for a 10,000-character
//! paragraph, and reports `2` for both.
//!
//! That left the tree's own memory claims uncheckable. `LayoutCache`'s ceiling
//! is documented as "~26 MB", derived from an entry count times a measured
//! *average* entry — a claim nothing can test, and one R1531 then weakened
//! further by adding cached draw lists to the entries the average was measured
//! over.
//!
//! # The contract
//!
//! [`Footprint::footprint`] returns the bytes of heap a value's containers
//! **report** holding, plus the footprints of what is in them:
//!
//! - `Vec<T>` — `capacity() * size_of::<T>()` plus each element's footprint.
//!   Capacity rather than length, because capacity is what was allocated.
//! - `String` — `capacity()`.
//! - `HashMap` / `HashSet` / `LruCache` — the container's own reported
//!   capacity times a stated per-slot model, plus the elements' footprints.
//!
//! It excludes two things by definition, and both exclusions are what make the
//! numbers add up rather than double-count:
//!
//! - **The value's own inline size.** A `Vec<PositionedRun>`'s 24 bytes belong
//!   to whatever holds the `Vec`, which counts them in its own `size_of`
//!   arithmetic. Counting them here would bill them twice.
//! - **Shared (`Arc` / `Rc`) interiors.** A `PositionedRun` carries a
//!   `FontData`, which is an `Arc`'d font file the collection owns; 500 runs
//!   over one face would otherwise report the same 5 MB five hundred times.
//!   Shared bytes are attributed to the arena that owns them, counted once
//!   there.
//!
//! # Why a trait with exhaustive destructuring, and not a formula
//!
//! Every implementation in this workspace destructures its type (`let Self { a, b, c } = self;`) and sums
//! the bindings. That is not a style preference — it is the whole guarantee. A
//! field added later to a cached struct **fails to compile** until someone
//! states what it contributes, so the number cannot silently start
//! under-counting. The toolkit's nearest equivalent, `sizeInBytes()`, is a formula (`bytesPerLine * height`)
//! with nothing tying it to the object's fields; a new buffer on the class
//! changes nothing about what it answers.
//!
//! # Against the toolkit 6.11
//!
//! The toolkit's floor is real but narrow. `setCacheLimit(int kb)` sets a
//! byte budget, `sizeInBytes()` and
//! `textureByteCount()` let an object state its own size.
//!
//! Two things here are past it:
//!
//! - **Usage, not just the budget.** pixmap cache has `cacheLimit()` and
//!   `clear()` and *no accessor for how much of the limit is in use* — a toolkit
//!   application cannot tell whether its pixmap cache sits at 1% or 99% of the
//!   ceiling it set. font cache, which is the closer analogue of the §5.36
//!   shape cache, is private in its entirety. Here every arena answers with
//!   what it is holding right now.
//! - **The accounting cannot silently rot.** See above: `sizeInBytes()` is a
//!   formula over two members; this is a total over every field, enforced by
//!   the compiler.
//!
//! # Allocated, not resident
//!
//! This is what the allocator was asked for. It is **not** what the OS has
//! made resident, and for a large freshly-allocated table the two differ a
//! lot: hashbrown writes one control byte per bucket at construction and
//! leaves the entry slots untouched, so most of its pages never fault in.
//!
//! Measured on a fresh `LayoutCache`, whose ghost index is sized to the
//! cache's ceiling: **139,264 bytes allocated** (this accounting) against the
//! **~33 kB** R1521 measured as an RSS delta over 100 of them. Neither number
//! is wrong — one is the reservation, the other the residency — and both are
//! published, because a report that gave only one would be read as the other.
//! `MemoryCensus::process_rss_bytes` is the resident half.
//!
//! # What it does not measure
//!
//! Allocator bookkeeping (malloc headers, arena slack) is outside every
//! container's reported capacity and so outside this. The interiors of foreign
//! opaque values are too — `parley::Layout` keeps its buffers behind a
//! `pub(crate)` field, so no API outside that crate can size them. An arena
//! holding such values reports how many it holds rather than pretending to
//! zero: see [`FootprintBasis`](crate::memory_census::FootprintBasis).

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hash};
use std::mem::size_of;

use lru::LruCache;

/// R1550 — the heap bytes a value uniquely owns.
///
/// See the module docs for the contract, which is load-bearing in two places:
/// capacity rather than length, and no shared (`Arc` / `Rc`) interiors.
pub trait Footprint {
    /// Bytes of heap this value's containers report holding, plus the
    /// footprints of the values in them. Excludes `size_of::<Self>()` — that
    /// belongs to whatever holds this value.
    fn footprint(&self) -> usize;
}

/// Bytes a `HashMap` / `HashSet` slot costs beyond the entry itself.
///
/// hashbrown keeps one control byte per bucket beside the entry array. The
/// bucket array is somewhat larger than the capacity the map reports (it
/// rounds to a power of two above `capacity * 8 / 7`), so a model built on
/// `capacity()` states the map's own number rather than reverse-engineering
/// its geometry — an under-statement of the table by that fixed factor, and a
/// stable one, which is why it is written here once rather than guessed at
/// each call site.
const HASH_SLOT_OVERHEAD: usize = 1;

/// The bytes a hash container's table holds for `capacity` slots of `T`.
#[must_use]
pub fn hash_table_bytes<T>(capacity: usize) -> usize {
    capacity.saturating_mul(size_of::<T>() + HASH_SLOT_OVERHEAD)
}

/// The bytes an [`LruCache`] holds for `entries` live nodes indexed at
/// `capacity` slots, excluding the keys' and values' own footprints.
///
/// `lru` boxes one node per live entry — key, value, and the two links that
/// make the recency list — and indexes them through a hash map sized to the
/// cache's capacity, holding a key reference and a node pointer per slot.
#[must_use]
pub fn lru_table_bytes<K, V>(entries: usize, capacity: usize) -> usize {
    let node = size_of::<K>() + size_of::<V>() + 2 * size_of::<*const ()>();
    entries
        .saturating_mul(node)
        .saturating_add(hash_table_bytes::<(*const K, *const V)>(capacity))
}

/// Types with no heap at all. Written once rather than per type so that a
/// primitive contributing zero is a fact stated in one place.
macro_rules! footprint_is_zero {
    ($($t:ty),* $(,)?) => {
        $(
            impl Footprint for $t {
                fn footprint(&self) -> usize {
                    0
                }
            }
        )*
    };
}

footprint_is_zero!(
    bool,
    char,
    f32,
    f64,
    i8,
    i16,
    i32,
    i64,
    isize,
    u8,
    u16,
    u32,
    u64,
    usize,
    ()
);

// The scene's plain style values. Listed rather than skipped at the call site
// so that summing every field of a struct that holds one compiles — which is
// what makes the destructuring fuse total: a field can only be left out of an
// accounting by deleting it from the pattern, never by forgetting it.
footprint_is_zero!(
    crate::style::Color,
    crate::style::FontStyle,
    crate::style::FontWeight,
    crate::style::GenericFontFamily,
    crate::style::LineHeight,
    crate::style::LetterSpacing,
    crate::style::BlockFormat,
    crate::style::TextAlign,
    crate::style::TextDecoration,
    crate::style::TextIndent,
    crate::style::TextOverflow,
    crate::style::UnderlineStyle,
);

footprint_is_zero!(
    std::num::NonZeroU32,
    std::num::NonZeroUsize,
    crate::reactive::SystemFontStatus,
    crate::scene::Rect,
);

impl Footprint for std::borrow::Cow<'static, str> {
    fn footprint(&self) -> usize {
        match self {
            Self::Borrowed(_) => 0,
            Self::Owned(s) => s.capacity(),
        }
    }
}

impl Footprint for crate::style::FontFamily {
    fn footprint(&self) -> usize {
        match self {
            Self::Named(name) => name.footprint(),
            Self::Generic(generic) => generic.footprint(),
        }
    }
}

impl Footprint for crate::style::TextStyle {
    fn footprint(&self) -> usize {
        let Self {
            font_family,
            font_size_px,
            fg_color,
            bg_color,
            font_weight,
            font_style,
            line_height,
            letter_spacing,
            text_align,
            text_indent,
            decoration,
            overflow,
        } = self;
        font_family.footprint()
            + font_size_px.footprint()
            + fg_color.footprint()
            + bg_color.footprint()
            + font_weight.footprint()
            + font_style.footprint()
            + line_height.footprint()
            + letter_spacing.footprint()
            + text_align.footprint()
            + text_indent.footprint()
            + decoration.footprint()
            + overflow.footprint()
    }
}

impl Footprint for crate::scene::StyleRun {
    fn footprint(&self) -> usize {
        let Self {
            start,
            end,
            style,
            name,
        } = self;
        start.footprint() + end.footprint() + style.footprint() + name.footprint()
    }
}

impl Footprint for String {
    fn footprint(&self) -> usize {
        self.capacity()
    }
}

impl<T: Footprint> Footprint for Vec<T> {
    fn footprint(&self) -> usize {
        self.capacity() * size_of::<T>() + self.iter().map(Footprint::footprint).sum::<usize>()
    }
}

impl<T: Footprint> Footprint for Option<T> {
    fn footprint(&self) -> usize {
        self.as_ref().map_or(0, Footprint::footprint)
    }
}

impl<T: Footprint> Footprint for Box<T> {
    fn footprint(&self) -> usize {
        size_of::<T>() + (**self).footprint()
    }
}

impl<K: Footprint, V: Footprint, S> Footprint for HashMap<K, V, S> {
    fn footprint(&self) -> usize {
        hash_table_bytes::<(K, V)>(self.capacity())
            + self
                .iter()
                .map(|(k, v)| k.footprint() + v.footprint())
                .sum::<usize>()
    }
}

impl<T: Footprint, S> Footprint for HashSet<T, S> {
    fn footprint(&self) -> usize {
        hash_table_bytes::<T>(self.capacity())
            + self.iter().map(Footprint::footprint).sum::<usize>()
    }
}

impl<K: Footprint + Hash + Eq, V: Footprint, S: BuildHasher> Footprint for LruCache<K, V, S> {
    fn footprint(&self) -> usize {
        lru_table_bytes::<K, V>(self.len(), self.cap().get())
            + self
                .iter()
                .map(|(k, v)| k.footprint() + v.footprint())
                .sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The contract's first half: a container reports its **capacity**, not
    /// its length. A `Vec` that grew and was drained still holds the buffer.
    #[test]
    fn r1550_capacity_not_length() {
        let mut v: Vec<u64> = Vec::with_capacity(64);
        assert_eq!(v.footprint(), 64 * 8, "an empty buffer is still allocated");
        v.push(1);
        assert_eq!(
            v.footprint(),
            64 * 8,
            "and pushing into it allocates nothing"
        );
    }

    /// The contract's second half: a value's own inline size belongs to
    /// whatever holds it, so a `Vec<String>` counts the string buffers plus
    /// its own element array, and nothing else.
    #[test]
    fn r1550_nested_heap_adds_to_the_element_array() {
        let v = vec![String::with_capacity(10), String::with_capacity(30)];
        assert_eq!(
            v.footprint(),
            2 * size_of::<String>() + 40,
            "two element slots plus the two string buffers",
        );
    }

    #[test]
    fn r1550_empty_containers_hold_nothing() {
        assert_eq!(Vec::<u8>::new().footprint(), 0);
        assert_eq!(String::new().footprint(), 0);
        assert_eq!(HashMap::<u32, u32>::new().footprint(), 0);
        assert_eq!(Option::<String>::None.footprint(), 0);
    }

    /// A footprint is monotone in what it holds. Not a tautology: a model
    /// built on the wrong side of a container's API (length where capacity was
    /// meant) can be flat across a growth that allocated.
    #[test]
    fn r1550_footprint_is_monotone_in_content() {
        let small: Vec<String> = (0..8).map(|i| format!("row {i}")).collect();
        let large: Vec<String> = (0..800).map(|i| format!("row {i}")).collect();
        assert!(
            large.footprint() > 50 * small.footprint(),
            "100x the rows is at least 50x the bytes: {} vs {}",
            large.footprint(),
            small.footprint(),
        );
    }

    #[test]
    fn r1550_lru_holds_its_index_at_capacity() {
        use std::num::NonZeroUsize;
        let cap = NonZeroUsize::new(128).expect("128 is non-zero");
        let empty: LruCache<u64, u64> = LruCache::new(cap);
        let mut full: LruCache<u64, u64> = LruCache::new(cap);
        for i in 0..128 {
            full.put(i, i);
        }
        assert!(
            empty.footprint() > 0,
            "an LRU allocates its index up front, so an empty one is not free",
        );
        assert!(
            full.footprint() > empty.footprint(),
            "and the nodes cost on top of it",
        );
    }
}
